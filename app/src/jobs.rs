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

#[derive(Clone)]
pub struct Ctx {
    pub vault: Arc<Mutex<Vault>>,
    pub file: Arc<Mutex<Option<VaultFile>>>,
    pub bridge: Bridge,
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
            let _ = ctx.bridge.weak.upgrade_in_event_loop(move |app| {
                let st = app.global::<AppState>();
                st.set_busy(false);
                st.set_show_pinpad(false);
                crate::refresh_view(&app);
                match (result, save) {
                    (Ok(msg), Ok(())) => crate::toast(&app, &msg, false),
                    (Err(e), _) | (Ok(_), Err(e)) => crate::toast(&app, &e, true),
                }
            });
        });
    }
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
        AccessKind::Demo => Err("Demo-Zugang".into()),
    }
}

fn make_client<'a>(ctx: &Ctx, access: &BankAccess, bridge: &'a Bridge) -> Result<Client<'a>, String> {
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
            if (30..=39).contains(&acc.kind) {
                continue; // securities accounts are out of scope
            }
            if allowed(acc, &["HKSAL"]) {
                match client.balance(acc) {
                    Ok(Some(b)) => {
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
            if allowed(acc, &["HKKAZ", "HKCAZ"]) {
                let from = ctx
                    .vault
                    .lock()
                    .unwrap()
                    .last_booking(id)
                    .map(|d| d - Duration::days(10))
                    .unwrap_or_else(|| Local::now().date_naive() - Duration::days(180));
                match client.transactions(acc, from, None) {
                    Ok(txs) => new_tx += ctx.vault.lock().unwrap().merge_transactions(id, txs),
                    Err(Error::NotSupported(_)) => {}
                    Err(Error::Cancelled) => return Err(Error::Cancelled),
                    Err(e) => warnings.push(format!("Umsätze {}: {}", fints::client::display_account(acc), err_text(&e))),
                }
            }
            let mut v = ctx.vault.lock().unwrap();
            if let Some(s) = v.accounts.iter_mut().find(|s| s.id == id) {
                s.last_sync = Some(Local::now().naive_local());
            }
        }
        Ok(accounts.len())
    })();
    let state = client.finish();
    store_state(ctx, access_id, state);
    let count = result.map_err(|e| err_text(&e))?;
    let mut msg = format!("{}: {} Konten aktualisiert, {} neue Umsätze.", access.name, count, new_tx);
    if !warnings.is_empty() {
        msg.push('\n');
        msg.push_str(&warnings.join("\n"));
    }
    Ok(msg)
}

pub fn sync_all(ctx: &Ctx) -> JobResult {
    let ids: Vec<u32> = ctx.vault.lock().unwrap().accesses.iter().map(|a| a.id).collect();
    if ids.is_empty() {
        return Err("Noch kein Bankzugang eingerichtet.".into());
    }
    let mut msgs = Vec::new();
    for id in ids {
        match sync_access(ctx, id) {
            Ok(m) => msgs.push(m),
            Err(e) if e.contains("abgebrochen") => return Err(e),
            Err(e) => msgs.push(e),
        }
    }
    Ok(msgs.join("\n"))
}

pub fn reset_tan(ctx: &Ctx, access_id: u32) -> JobResult {
    let mut access = ctx.vault.lock().unwrap().access(access_id).cloned().ok_or("Bankzugang nicht gefunden")?;
    access.state.tan_method = None;
    access.state.tan_medium = None;
    let bridge = ctx.bridge.clone();
    let mut client = make_client(ctx, &access, &bridge)?;
    let r = client.open();
    let state = client.finish();
    let name = state
        .tan_method
        .as_ref()
        .and_then(|sf| state.bpd.tan_methods.iter().find(|m| &m.security_function == sf).map(|m| m.name.clone()));
    store_state(ctx, access_id, state);
    r.map_err(|e| err_text(&e))?;
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
    let state = client.finish();
    store_state(ctx, access.id, state);
    let summary = r.map_err(|e| err_text(&e))?;
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
    let mut warnings = Vec::new();
    for (access, accs) in groups {
        if accs.is_empty() || !access.state.bpd.supports("HKCDB") && !access.state.bpd.versions.is_empty() {
            continue;
        }
        let bridge = ctx.bridge.clone();
        let mut client = make_client(ctx, &access, &bridge)?;
        for (id, acc) in accs {
            match client.standing_orders(&acc) {
                Ok(list) => {
                    total += list.len();
                    let mut v = ctx.vault.lock().unwrap();
                    v.standing.retain(|s| s.account_id != id);
                    v.standing.extend(list.into_iter().map(|order| StoredStanding { account_id: id, order }));
                }
                Err(Error::Cancelled) => {
                    let st = client.finish();
                    store_state(ctx, access.id, st);
                    return Err("Vorgang abgebrochen.".into());
                }
                Err(e) => warnings.push(err_text(&e)),
            }
        }
        let st = client.finish();
        store_state(ctx, access.id, st);
    }
    let mut msg = format!("{total} Daueraufträge geladen.");
    if !warnings.is_empty() {
        msg.push('\n');
        msg.push_str(&warnings.join("\n"));
    }
    Ok(msg)
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
    let r2 = r.and_then(|s| client.standing_orders(&acc).map(|l| (s, l)));
    let state = client.finish();
    store_state(ctx, access.id, state);
    let (summary, list) = r2.map_err(|e| err_text(&e))?;
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
        let state = client.finish();
        store_state(ctx, access.id, state);
        r.map_err(|e| err_text(&e))?;
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
