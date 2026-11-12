//! Background jobs talking to banks. Each job runs on its own thread; the UI stays responsive
//! and dialogs (PIN, TAN, method selection) are served through the [`Bridge`].

use crate::bridge::Bridge;
use crate::demo;
use crate::store::{AccessKind, BankAccess, StoredStanding, Vault, VaultFile};
use crate::view;
use crate::AppState;
use chrono::{Duration, Local};
use fints::{Account, Client, Error, Medium, SepaTransfer, StandingOrder, TanChallenge, TanMethod};
use slint::{ComponentHandle, SharedString};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone)]
pub struct Ctx {
    pub vault: Arc<Mutex<Vault>>,
    pub file: Arc<Mutex<Option<VaultFile>>>,
    pub bridge: Bridge,
    pub running: Arc<AtomicBool>,
}

pub type JobResult = Result<String, String>;

impl Ctx {
    pub fn save(&self) -> Result<(), String> {
        let v = self.vault.lock().unwrap();
        match self.file.lock().unwrap().as_ref() {
            Some(f) => f.save(&v),
            None => Ok(()),
        }
    }

    /// Run `f` on a worker thread with the busy overlay, then save and refresh.
    pub fn run(&self, title: &str, f: impl FnOnce(&Ctx) -> JobResult + Send + 'static) {
        // Reserve synchronously before the busy overlay is posted to the event loop.
        if self.running.swap(true, Ordering::AcqRel) {
            return;
        }
        let ctx = self.clone();
        let title = SharedString::from(title);
        let _ = self.bridge.weak.upgrade_in_event_loop(move |app| {
            let st = app.global::<AppState>();
            st.set_busy(true);
            st.set_busy_text(title);
        });
        std::thread::spawn(move || {
            *ctx.bridge.pin_cache.lock().unwrap() = None;
            let result = f(&ctx);
            *ctx.bridge.pin_cache.lock().unwrap() = None;
            let save = ctx.save();
            ctx.running.store(false, Ordering::Release);
            let _ = ctx.bridge.weak.upgrade_in_event_loop(move |app| {
                let st = app.global::<AppState>();
                st.set_busy(false);
                st.set_show_pinpad(false);
                crate::refresh_view(&app);
                match (result, save) {
                    (Ok(msg), Ok(())) => crate::toast(&app, &msg, false),
                    (Ok(msg), Err(e)) => crate::toast(&app, &format!("{msg}\n\nLokales Speichern fehlgeschlagen: {e}"), true),
                    (Err(e), Ok(())) => crate::toast(&app, &e, true),
                    (Err(e), Err(save)) => crate::toast(&app, &format!("{e}\n\nLokales Speichern fehlgeschlagen: {save}"), true),
                }
            });
        });
    }
}

/// Store the client state and turn a FinTS error into a user message; bank/syntax errors
/// get a masked protocol of the last message exchange written next to the vault.
fn conclude<T>(ctx: &Ctx, access_id: u32, client: Client, r: Result<T, Error>) -> Result<T, String> {
    let trace = client.last_trace.clone();
    let state = client.finish();
    store_state(ctx, access_id, state);
    let wrong_pin = matches!(&r, Err(Error::Bank { code, text })
        if ["9931", "9942", "9340", "9930"].iter().any(|c| code == c || text.contains(c)));
    let pin_note = handle_saved_pin(ctx, access_id, wrong_pin);
    let r = r.inspect(|_| {
        if let Some(n) = &pin_note {
            log::info!("{n}");
        }
    });
    r.map_err(|e| {
        let mut msg = err_text(&e);
        if matches!(e, Error::Bank { .. } | Error::Syntax(_)) {
            if let Some(p) = write_error_report(trace.as_deref()) {
                msg.push_str(&format!("\n\nFehlerprotokoll (ohne Nachrichteninhalte): {}", p.display()));
            }
        }
        msg
    })
}

/// Store the PIN when the user asked for it, drop a stored PIN the bank rejected.
fn handle_saved_pin(ctx: &Ctx, access_id: u32, wrong_pin: bool) -> Option<String> {
    let bridge = &ctx.bridge;
    let mut v = ctx.vault.lock().unwrap();
    let access = v.accesses.iter_mut().find(|a| a.id == access_id)?;
    if wrong_pin {
        *bridge.pin_cache.lock().unwrap() = None;
        *bridge.pin_save_requested.lock().unwrap() = false;
        if access.saved_pin.take().is_some() {
            return Some("Die gespeicherte PIN wurde von der Bank abgelehnt und gelöscht.".into());
        }
        return None;
    }
    if std::mem::take(&mut *bridge.pin_save_requested.lock().unwrap()) {
        let pin = bridge.pin_cache.lock().unwrap().clone()?;
        let sealed = ctx.file.lock().unwrap().as_ref()?.seal_secret(&pin).ok()?;
        access.saved_pin = Some(sealed);
        return Some("PIN verschlüsselt gespeichert.".into());
    }
    None
}

fn write_error_report(trace: Option<&str>) -> Option<std::path::PathBuf> {
    let trace = trace?;
    let dir = crate::store::default_path().parent()?.to_path_buf();
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("fints-fehlerprotokoll.txt");
    let text = format!(
        "BankingCraft {} – {}\n\nTechnische Segmentübersicht (Fehlerdetails werden nur in der App angezeigt):\n{}\n",
        env!("CARGO_PKG_VERSION"),
        Local::now().format("%d.%m.%Y %H:%M:%S"),
        trace.replace('\'', "'\n")
    );
    crate::private_file::replace(&path, text.as_bytes()).ok()?;
    Some(path)
}

fn err_text(e: &Error) -> String {
    match e {
        Error::Cancelled => "Vorgang abgebrochen.".into(),
        Error::Bank { text, .. } => format!("Die Bank meldet:\n{text}"),
        Error::Transport(t) => format!("Verbindung zur Bank fehlgeschlagen: {t}"),
        Error::NotSupported(t) => format!("Von der Bank nicht angeboten: {t}"),
        other => other.to_string(),
    }
}

fn medium_for(ctx: &Ctx, access: &BankAccess) -> Result<Medium, String> {
    match &access.kind {
        AccessKind::PinTan => Ok(Medium::PinTan),
        AccessKind::ChipCard { reader, .. } => {
            let b = ctx.bridge.clone();
            let soft: cardreader::SoftPinProvider = Box::new(move || b.ask_pin("PIN der HBCI-Chipkarte (Leser ohne PIN-Pad)"));
            ctx.bridge.status("Verbinde mit Chipkarte …");
            let mut card = cardreader::DdvChipCard::open(Some(reader), Some(soft)).map_err(|e| e.to_string())?;
            let b2 = ctx.bridge.clone();
            card.pinpad_notify = Some(Box::new(move |on| {
                b2.pinpad(on, "Bitte geben Sie die PIN Ihrer HBCI-Chipkarte am Kartenleser ein und bestätigen Sie mit OK.");
            }));
            Ok(Medium::Ddv(Box::new(card)))
        }
        AccessKind::RdhCard { reader, record } => {
            let card = open_rdh_card(ctx, reader)?;
            Ok(Medium::Rdh(Box::new(card), *record))
        }
        AccessKind::Demo => Err("Demo-Zugang".into()),
    }
}

fn open_rdh_card(ctx: &Ctx, reader: &str) -> Result<cardreader::RdhChipCard, String> {
    let b = ctx.bridge.clone();
    let soft: cardreader::SoftPinProvider = Box::new(move || b.ask_pin("PIN der RDH-Chipkarte (Leser ohne PIN-Pad)"));
    ctx.bridge.status("Verbinde mit RDH-Chipkarte …");
    let mut card = cardreader::RdhChipCard::open(Some(reader), Some(soft)).map_err(|e| e.to_string())?;
    let b2 = ctx.bridge.clone();
    card.pinpad_notify = Some(Box::new(move |on| {
        b2.pinpad(on, "Bitte geben Sie die PIN Ihrer Chipkarte (z. B. VR-NetWorld-Card) am Kartenleser ein und bestätigen Sie mit OK.");
    }));
    Ok(card)
}

/// Read the bank access `record` (0 = first) from the notepad of an RDH card.
pub fn read_rdh_card(ctx: &Ctx, reader: &str, record: u8) -> Result<(fints::rdh::RdhContext, String), String> {
    use fints::rdh::RdhCard;
    let mut card = open_rdh_card(ctx, reader)?;
    let number = card.card_number();
    let list = card.contexts().map_err(|e| err_text(&e))?;
    let c = list
        .iter()
        .find(|c| record == 0 || c.record == record)
        .cloned()
        .ok_or("Auf der Chipkarte ist kein Bankzugang eingerichtet")?;
    Ok((c, number))
}

fn make_client<'a>(ctx: &Ctx, access: &BankAccess, bridge: &'a Bridge) -> Result<Client<'a>, String> {
    // saved PIN (decrypted only in memory) and the offer to save it
    let pintan = access.kind == AccessKind::PinTan;
    *bridge.pin_save_offer.lock().unwrap() = pintan && access.saved_pin.is_none();
    *bridge.pin_save_requested.lock().unwrap() = false;
    let saved = if pintan {
        access
            .saved_pin
            .as_ref()
            .and_then(|sp| ctx.file.lock().unwrap().as_ref().and_then(|f| f.open_secret(sp)))
    } else {
        None
    };
    *bridge.pin_cache.lock().unwrap() = saved;
    let medium = medium_for(ctx, access)?;
    let mut cfg = access.config.clone();
    {
        let v = ctx.vault.lock().unwrap();
        cfg.product_id = v.product_id.clone();
        cfg.product_version = v.product_version.clone();
    }
    Client::new(cfg, access.state.clone(), medium, bridge).map_err(|e| err_text(&e))
}

fn store_state(ctx: &Ctx, access_id: u32, state: fints::State) {
    let mut v = ctx.vault.lock().unwrap();
    if let Some(a) = v.accesses.iter_mut().find(|a| a.id == access_id) {
        a.state = state;
    }
}

fn allowed(acc: &Account, codes: &[&str]) -> bool {
    acc.allowed.is_empty() || codes.iter().any(|c| acc.allowed.iter().any(|a| a == c))
}

/// Kontenrundruf for one access: accounts, balances, transactions.
pub fn sync_access(ctx: &Ctx, access_id: u32) -> JobResult {
    let access = ctx.vault.lock().unwrap().access(access_id).cloned().ok_or("Bankzugang nicht gefunden")?;
    if access.kind == AccessKind::Demo {
        return Ok(format!("{}: Demo-Daten sind aktuell.", access.name));
    }
    let bridge = ctx.bridge.clone();
    let mut client = make_client(ctx, &access, &bridge)?;
    let mut warnings: Vec<String> = Vec::new();
    let mut new_tx = 0usize;
    let result: Result<usize, Error> = (|| {
        let accounts = client.accounts()?;
        let ids = ctx.vault.lock().unwrap().merge_accounts(access_id, accounts.clone());
        for (acc, id) in accounts.iter().zip(ids) {
            let warnings_before = warnings.len();
            let mut fetched = false;
            if (30..=39).contains(&acc.kind) {
                continue; // securities accounts are out of scope
            }
            if allowed(acc, &["HKSAL", "HKKKS", "DKKKS"]) {
                match client.balance(acc) {
                    Ok(Some(b)) => {
                        fetched = true;
                        let mut v = ctx.vault.lock().unwrap();
                        if let Some(s) = v.accounts.iter_mut().find(|s| s.id == id) {
                            s.balance = Some(b);
                        }
                    }
                    Ok(None) | Err(Error::NotSupported(_)) => {}
                    Err(Error::Cancelled) => return Err(Error::Cancelled),
                    Err(e) => warnings.push(format!("Saldo {}: {}", fints::client::display_account(acc), err_text(&e))),
                }
            }
            if allowed(acc, &["HKKAZ", "HKCAZ", "HKKKU", "DKKKU"]) {
                let from = ctx
                    .vault
                    .lock()
                    .unwrap()
                    .last_booking(id)
                    .map(|d| d - Duration::days(10))
                    // PSD2: up to 90 days of history need no extra approval
                    .unwrap_or_else(|| Local::now().date_naive() - Duration::days(89));
                match client.transaction_report(acc, from, None) {
                    Ok(report) => {
                        fetched = true;
                        let mut v = ctx.vault.lock().unwrap();
                        if let Some(balance) = report.balance {
                            if let Some(stored) = v.accounts.iter_mut().find(|s| s.id == id) { stored.balance = Some(balance); }
                        }
                        new_tx += v.merge_transactions(id, report.transactions);
                    }
                    Err(Error::NotSupported(e)) if (50..=59).contains(&acc.kind) => warnings.push(format!("Kreditkartenumsätze: {e}")),
                    Err(Error::NotSupported(_)) => {}
                    Err(Error::Cancelled) => warnings.push(format!(
                        "Umsätze {}: Freigabe abgebrochen – Umsätze wurden nicht abgerufen.",
                        fints::client::display_account(acc)
                    )),
                    Err(e) => {
                        let message = format!("Umsätze {}: {}", fints::client::display_account(acc), err_text(&e));
                        if matches!(e, Error::Bank { .. } | Error::Syntax(_)) {
                            write_error_report(client.last_trace.as_deref());
                        }
                        warnings.push(message);
                    },
                }
            }
            let mut v = ctx.vault.lock().unwrap();
            if let Some(s) = v.accounts.iter_mut().find(|s| s.id == id) {
                if fetched && warnings.len() == warnings_before { s.last_sync = Some(Local::now().naive_local()); }
            }
        }
        Ok(accounts.len())
    })();
    let count = conclude(ctx, access_id, client, result)?;
    let mut msg = format!("{}: {} Konten geprüft, {} neue Umsätze.", access.name, count, new_tx);
    if !warnings.is_empty() {
        msg.push('\n');
        msg.push_str(&warnings.join("\n"));
    }
    if warnings.is_empty() { Ok(msg) } else { Err(msg) }
}

pub fn sync_all(ctx: &Ctx) -> JobResult {
    let ids: Vec<u32> = ctx.vault.lock().unwrap().accesses.iter().map(|a| a.id).collect();
    if ids.is_empty() {
        return Err("Noch kein Bankzugang eingerichtet.".into());
    }
    let mut msgs = Vec::new();
    let mut failed = false;
    for id in ids {
        match sync_access(ctx, id) {
            Ok(m) => msgs.push(m),
            Err(e) if e.contains("abgebrochen") => return Err(e),
            Err(e) => { failed = true; msgs.push(e); },
        }
    }
    if failed { Err(msgs.join("\n")) } else { Ok(msgs.join("\n")) }
}

pub fn reset_tan(ctx: &Ctx, access_id: u32) -> JobResult {
    let mut access = ctx.vault.lock().unwrap().access(access_id).cloned().ok_or("Bankzugang nicht gefunden")?;
    access.state.tan_method = None;
    access.state.tan_medium = None;
    let bridge = ctx.bridge.clone();
    let mut client = make_client(ctx, &access, &bridge)?;
    let r = client.open();
    conclude(ctx, access_id, client, r)?;
    let name = ctx.vault.lock().unwrap().access(access_id).and_then(|a| {
        let sf = a.state.tan_method.clone()?;
        a.state.bpd.tan_methods.iter().find(|m| m.security_function == sf).map(|m| m.name.clone())
    });
    Ok(format!("TAN-Verfahren: {}", name.unwrap_or_else(|| "Standard".into())))
}

fn account_and_access(ctx: &Ctx, account_id: u32) -> Result<(Account, BankAccess), String> {
    let v = ctx.vault.lock().unwrap();
    let a = v.account(account_id).ok_or("Konto nicht gefunden")?;
    let access = v.access(a.access_id).cloned().ok_or("Bankzugang nicht gefunden")?;
    let mut acc = a.account.clone();
    if acc.blz.is_empty() {
        acc.blz = access.config.blz.clone();
    }
    Ok((acc, access))
}

fn demo_tan(ctx: &Ctx, what: &str) -> Result<(), String> {
    use fints::Interaction;
    let ch = TanChallenge {
        text: format!("Demo: {what}\n\nIm Demo-Modus wird nichts an eine Bank gesendet. Geben Sie eine beliebige TAN ein."),
        hhduc: Some(demo::DEMO_FLICKER.as_bytes().to_vec()),
        method: TanMethod {
            security_function: "Demo".into(),
            name: "chipTAN optisch (Demo)".into(),
            technical_id: "HHD1.4OPT".into(),
            dk_id: "HHD".into(),
            tan_process: "2".into(),
            decoupled: false,
            needs_medium: false,
            max_tan_length: 6,
            hhduc_response_required: false,
            decoupled_poll_wait_first: 0,
            decoupled_poll_wait_next: 0,
        },
        decoupled: false,
        task_reference: "DEMO".into(),
    };
    match ctx.bridge.tan(&ch) {
        fints::TanAnswer::Cancel => Err("Vorgang abgebrochen.".into()),
        _ => Ok(()),
    }
}

pub fn transfer(ctx: &Ctx, account_id: u32, t: SepaTransfer, scheduled: bool) -> JobResult {
    let (acc, access) = account_and_access(ctx, account_id)?;
    let mut t = t;
    t.debtor_iban = acc.iban.clone();
    t.debtor_bic = acc.bic.clone();
    if t.debtor_name.is_empty() {
        t.debtor_name = if acc.owner.is_empty() { "Kontoinhaber".into() } else { acc.owner.clone() };
    }
    if access.kind == AccessKind::Demo {
        demo_tan(ctx, &format!("Überweisung {} an {}", view::fmt_eur(t.amount), t.creditor_name))?;
        let mut tx = fints::Transaction {
            booking_date: t.execution_date.unwrap_or_else(|| Local::now().date_naive()),
            value_date: t.execution_date.unwrap_or_else(|| Local::now().date_naive()),
            amount: -t.amount,
            currency: "EUR".into(),
            counterparty_name: t.creditor_name.clone(),
            counterparty_iban: t.creditor_iban.clone(),
            purpose: t.purpose.clone(),
            posting_text: if t.instant { "ECHTZEIT-ÜBERWEISUNG" } else { "ÜBERWEISUNG" }.into(),
            pending: true,
            ..Default::default()
        };
        tx.compute_fingerprint();
        let mut v = ctx.vault.lock().unwrap();
        v.transactions.push(crate::store::StoredTx { account_id, tx, category: None });
        return Ok("Demo: Überweisung wurde simuliert und als vorgemerkter Umsatz eingetragen.".into());
    }
    let bridge = ctx.bridge.clone();
    let mut client = make_client(ctx, &access, &bridge)?;
    let r = if scheduled { client.scheduled_transfer(&acc, &t) } else { client.transfer(&acc, &t) };
    let summary = conclude(ctx, access.id, client, r)?;
    Ok(format!("Auftrag übermittelt.\n{summary}"))
}

pub fn fetch_standing(ctx: &Ctx) -> JobResult {
    let groups: Vec<(BankAccess, Vec<(u32, Account)>)> = {
        let v = ctx.vault.lock().unwrap();
        v.accesses
            .iter()
            .filter(|a| a.kind != AccessKind::Demo)
            .map(|a| {
                let accs = v
                    .accounts
                    .iter()
                    .filter(|s| s.access_id == a.id && s.account.is_sepa() && allowed(&s.account, &["HKCDB"]))
                    .map(|s| (s.id, s.account.clone()))
                    .collect();
                (a.clone(), accs)
            })
            .collect()
    };
    if groups.is_empty() {
        return Ok("Nur Demo-Daueraufträge vorhanden.".into());
    }
    let mut total = 0;
    let mut loaded_accounts = 0;
    let mut warnings = Vec::new();
    for (access, accs) in groups {
        if accs.is_empty() || !access.state.bpd.supports("HKCDB") && !access.state.bpd.versions.is_empty() {
            continue;
        }
        let bridge = ctx.bridge.clone();
        let mut client = match make_client(ctx, &access, &bridge) {
            Ok(client) => client,
            Err(e) => { warnings.push(format!("{}: {e}", access.name)); continue; },
        };
        let mut wrong_pin = false;
        for (id, acc) in accs {
            match client.standing_orders(&acc) {
                Ok(list) => {
                    total += list.len();
                    loaded_accounts += 1;
                    let mut v = ctx.vault.lock().unwrap();
                    v.standing.retain(|s| s.account_id != id);
                    v.standing.extend(list.into_iter().map(|order| StoredStanding { account_id: id, order }));
                }
                Err(Error::Cancelled) => {
                    let st = client.finish();
                    store_state(ctx, access.id, st);
                    return Err("Vorgang abgebrochen.".into());
                }
                Err(e) => {
                    wrong_pin = matches!(&e, Error::Bank { code, text }
                        if ["9931", "9942", "9340", "9930"].iter().any(|c| code == c || text.contains(c)));
                    let msg = format!("{} · {}: {}", access.name, acc.product, err_text(&e));
                    if matches!(e, Error::Bank { .. } | Error::Syntax(_)) {
                        write_error_report(client.last_trace.as_deref());
                    }
                    warnings.push(msg);
                    if wrong_pin { break; }
                },
            }
        }
        let st = client.finish();
        store_state(ctx, access.id, st);
        handle_saved_pin(ctx, access.id, wrong_pin);
    }
    if !warnings.is_empty() {
        let status = if loaded_accounts == 0 {
            "Daueraufträge konnten nicht abgerufen werden.".to_string()
        } else {
            format!("Abruf teilweise erfolgreich: {total} Daueraufträge aus {loaded_accounts} Konten geladen.")
        };
        return Err(format!("{status}\n{}\nVorhandene Daueraufträge der betroffenen Konten bleiben erhalten.", warnings.join("\n")));
    }
    if loaded_accounts == 0 {
        return Ok("Keines der eingerichteten Konten bietet den Dauerauftragsabruf an.".into());
    }
    Ok(format!("{total} Daueraufträge geladen."))
}

pub fn create_standing(ctx: &Ctx, account_id: u32, mut so: StandingOrder) -> JobResult {
    let (acc, access) = account_and_access(ctx, account_id)?;
    so.transfer.debtor_iban = acc.iban.clone();
    so.transfer.debtor_bic = acc.bic.clone();
    if so.transfer.debtor_name.is_empty() {
        so.transfer.debtor_name = if acc.owner.is_empty() { "Kontoinhaber".into() } else { acc.owner.clone() };
    }
    if access.kind == AccessKind::Demo {
        demo_tan(ctx, &format!("Dauerauftrag {} an {}", view::fmt_eur(so.transfer.amount), so.transfer.creditor_name))?;
        so.order_id = format!("DEMO-{}", ctx.vault.lock().unwrap().new_id());
        ctx.vault.lock().unwrap().standing.push(StoredStanding { account_id, order: so });
        return Ok("Demo: Dauerauftrag angelegt.".into());
    }
    let bridge = ctx.bridge.clone();
    let mut client = make_client(ctx, &access, &bridge)?;
    let r = client.create_standing_order(&acc, &so);
    // Failure to refresh the list must not make an accepted order look unsuccessful.
    let r2 = r.map(|s| (s, client.standing_orders(&acc)));
    let (summary, list) = conclude(ctx, access.id, client, r2)?;
    let list = match list {
        Ok(list) => list,
        Err(e) => return Ok(format!("Dauerauftrag angelegt.\n{summary}\n\nDie Liste konnte nicht aktualisiert werden: {}. Bitte die Liste erneut abrufen, den Auftrag nicht erneut anlegen.", err_text(&e))),
    };
    let mut v = ctx.vault.lock().unwrap();
    v.standing.retain(|s| s.account_id != account_id);
    v.standing.extend(list.into_iter().map(|order| StoredStanding { account_id, order }));
    Ok(format!("Dauerauftrag angelegt.\n{summary}"))
}

pub fn delete_standing(ctx: &Ctx, index: usize) -> JobResult {
    let item = ctx.vault.lock().unwrap().standing.get(index).cloned().ok_or("Dauerauftrag nicht gefunden")?;
    let (acc, access) = account_and_access(ctx, item.account_id)?;
    if access.kind != AccessKind::Demo {
        let bridge = ctx.bridge.clone();
        let mut client = make_client(ctx, &access, &bridge)?;
        let r = client.delete_standing_order(&acc, &item.order);
        conclude(ctx, access.id, client, r)?;
    } else {
        demo_tan(ctx, &format!("Dauerauftrag an {} löschen", item.order.transfer.creditor_name))?;
    }
    let mut v = ctx.vault.lock().unwrap();
    v.standing.retain(|s| !(s.account_id == item.account_id && s.order.order_id == item.order.order_id));
    Ok("Dauerauftrag gelöscht.".into())
}

pub fn read_card(reader: &str, record: u8) -> Result<fints::security::DdvBankData, String> {
    use fints::security::DdvCard;
    let mut card = cardreader::DdvChipCard::open(Some(reader), None).map_err(|e| e.to_string())?;
    card.bank_data(record).map_err(|e| e.to_string())
}

#[cfg(test)]
mod business_tests {
    use super::*;
    fn context() -> Ctx {
        Ctx { vault: Arc::new(Mutex::new(Vault::default())), file: Arc::new(Mutex::new(None)),
            bridge: Bridge::new(slint::Weak::default()), running: Arc::new(AtomicBool::new(false)) }
    }
    #[test]
    fn demo_setup_and_refresh_are_idempotent_and_never_contact_a_bank() {
        let ctx = context(); demo::add_demo(&mut ctx.vault.lock().unwrap());
        let before = serde_json::to_vec(&*ctx.vault.lock().unwrap()).unwrap();
        demo::add_demo(&mut ctx.vault.lock().unwrap()); assert!(sync_all(&ctx).unwrap().contains("Demo"));
        assert!(fetch_standing(&ctx).unwrap().contains("Demo"));
        assert_eq!(serde_json::to_vec(&*ctx.vault.lock().unwrap()).unwrap(), before);
        assert!(ctx.vault.lock().unwrap().accounts.iter().all(|a| fints::pain::iban_valid(&a.account.iban)));
    }
    #[test]
    fn missing_access_account_or_order_fails_before_prompting_or_mutating() {
        let ctx = context(); assert!(sync_all(&ctx).is_err()); assert!(sync_access(&ctx, 999).is_err());
        assert!(reset_tan(&ctx, 999).is_err()); assert!(delete_standing(&ctx, 999).is_err());
        let so = crate::business_tests::standing(); assert!(transfer(&ctx, 999, so.transfer.clone(), false).is_err());
        assert!(create_standing(&ctx, 999, so).is_err()); assert!(ctx.vault.lock().unwrap().accounts.is_empty());
    }
    #[test]
    fn account_lookup_uses_access_bank_code_without_mutating_stored_account() {
        let ctx = context(); demo::add_demo(&mut ctx.vault.lock().unwrap());
        let id = { let mut v = ctx.vault.lock().unwrap(); v.accounts[0].account.blz.clear(); v.accounts[0].id };
        let (a, access) = account_and_access(&ctx, id).unwrap(); assert_eq!(a.blz, access.config.blz);
        assert!(ctx.vault.lock().unwrap().account(id).unwrap().account.blz.is_empty());
    }
    #[test]
    fn pin_storage_requires_explicit_choice_and_wrong_pin_removes_saved_secret() {
        let ctx = context(); demo::add_demo(&mut ctx.vault.lock().unwrap()); let id = ctx.vault.lock().unwrap().accesses[0].id;
        let path = std::env::temp_dir().join(format!("bankingcraft-pin-test-{:016x}", rand::random::<u64>()));
        *ctx.file.lock().unwrap() = Some(VaultFile::create(path, "synthetic-test-password").unwrap());
        *ctx.bridge.pin_cache.lock().unwrap() = Some(zeroize::Zeroizing::new("fixture-pin".into()));
        assert!(handle_saved_pin(&ctx, id, false).is_none()); assert!(ctx.vault.lock().unwrap().access(id).unwrap().saved_pin.is_none());
        *ctx.bridge.pin_save_requested.lock().unwrap() = true; assert!(handle_saved_pin(&ctx, id, false).is_some());
        let sealed = ctx.vault.lock().unwrap().access(id).unwrap().saved_pin.clone().unwrap();
        assert_eq!(&**ctx.file.lock().unwrap().as_ref().unwrap().open_secret(&sealed).as_ref().unwrap(), "fixture-pin");
        assert!(!*ctx.bridge.pin_save_requested.lock().unwrap());
        *ctx.bridge.pin_save_requested.lock().unwrap() = true; assert!(handle_saved_pin(&ctx, id, true).is_some());
        assert!(ctx.vault.lock().unwrap().access(id).unwrap().saved_pin.is_none()); assert!(!*ctx.bridge.pin_save_requested.lock().unwrap());
        assert!(ctx.bridge.pin_cache.lock().unwrap().is_none());
    }
}
