//! Background jobs talking to banks. Each job runs on its own thread; the UI stays responsive
//! and dialogs (PIN, TAN, method selection) are served through the [`Bridge`].

use crate::bridge::Bridge;
use crate::demo;
use crate::store::{AccessKind, BankAccess, StoredStanding, Vault, VaultFile};
use crate::view;
use crate::payments::{OrderStatus, PaymentOrder};
use crate::AppState;
use chrono::{Duration, Local};
use banking::{Account, Error, SepaTransfer, StandingOrder, TanChallenge, TanMethod};
use aqbanking::{Client, Medium};
use crate::ui::{SharedString};
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

#[cfg(test)]
mod account_refresh_tests;

/// The production implementation delegates all bank communication to AqBanking.
/// Keeping this small interface also permits socket-free refresh regressions.
trait RefreshClient: Sized {
    fn prepare_reads(&mut self, _requests:Vec<aqbanking::ReadRequest>) -> banking::Result<bool> {Ok(false)}
    fn accounts(&mut self) -> banking::Result<Vec<Account>>;
    fn current_accounts(&mut self, accounts: Vec<Account>) -> banking::Result<Vec<Account>> { Ok(accounts) }
    fn balance(&mut self, account: &Account) -> banking::Result<Option<banking::Balance>>;
    fn transaction_report(&mut self, account: &Account, from: chrono::NaiveDate, to: Option<chrono::NaiveDate>) -> banking::Result<banking::TransactionReport>;
    fn finish(self) -> banking::State;
}
macro_rules! refresh_client {
    ($client:ty) => {
        impl RefreshClient for $client {
            fn prepare_reads(&mut self, r:Vec<aqbanking::ReadRequest>) -> banking::Result<bool> { self.prepare_reads(r) }
            fn accounts(&mut self) -> banking::Result<Vec<Account>> { self.accounts() }
            fn current_accounts(&mut self, a: Vec<Account>) -> banking::Result<Vec<Account>> { self.current_accounts(a) }
            fn balance(&mut self, a: &Account) -> banking::Result<Option<banking::Balance>> { self.balance(a) }
            fn transaction_report(&mut self, a: &Account, f: chrono::NaiveDate, t: Option<chrono::NaiveDate>) -> banking::Result<banking::TransactionReport> { self.transaction_report(a,f,t) }
            fn finish(self) -> banking::State { self.finish() }
        }
    };
}
refresh_client!(Client<'_>);

impl Ctx {
    pub fn record_activity(&self, action: &'static str, status: crate::history::Status) {
        if self.file.lock().unwrap().is_none() {return;}
        let id=self.vault.lock().unwrap().history.add(action,status,None);
        if self.save().is_err() { self.vault.lock().unwrap().history.save_failed(id); }
    }

    pub fn edit_vault_logged<T>(&self, action: &'static str, edit: impl FnOnce(&mut Vault) -> Result<T, String>) -> Result<T, String> {
        let result=self.edit_vault(|v| {
            let result=edit(v)?;
            let transfer=if action=="Überweisung gespeichert" {
                v.payment_orders.last().map(|o|crate::history::Transfer::from_order(v,o))
            } else {None};
            v.history.add(action,crate::history::Status::Success,transfer);
            Ok(result)
        });
        if result.is_err() { self.record_activity(action,crate::history::Status::Error); }
        result
    }

    /// Commit a local edit only after the encrypted vault was saved successfully.
    pub fn edit_vault<T>(&self, edit: impl FnOnce(&mut Vault) -> Result<T, String>) -> Result<T, String> {
        let mut vault = self.vault.lock().unwrap();
        let mut candidate = vault.clone();
        let result = edit(&mut candidate)?;
        self.file.lock().unwrap().as_ref().ok_or(i18n::tr("Tresor ist gesperrt"))?.save(&candidate)?;
        *vault = candidate;
        Ok(result)
    }

    pub fn save(&self) -> Result<(), String> {
        let v = self.vault.lock().unwrap();
        match self.file.lock().unwrap().as_ref() {
            Some(f) => f.save(&v),
            None => Ok(()),
        }
    }

    /// Run `f` on a worker thread with the busy overlay, then save and refresh.
    pub fn run(&self, title: &'static str, f: impl FnOnce(&Ctx) -> JobResult + Send + 'static) {
        self.run_job(title, false, f);
    }
    pub fn run_sync(&self, title: &'static str, f: impl FnOnce(&Ctx) -> JobResult + Send + 'static) {
        self.run_job(title, true, f);
    }
    fn run_job(&self, title: &'static str, notify_sync: bool, f: impl FnOnce(&Ctx) -> JobResult + Send + 'static) {
        if self.bridge.locked.load(Ordering::Acquire) { return; }
        // Reserve synchronously before the busy overlay is posted to the event loop.
        if self.running.swap(true, Ordering::AcqRel) {
            return;
        }
        self.bridge.cancelled.store(false, Ordering::Release);
        let ctx = self.clone();
        let activity=ctx.vault.lock().unwrap().history.add(title,crate::history::Status::Running,None);
        let title = SharedString::from(i18n::tr(title));
        let locked = self.bridge.locked.clone();
        let _ = self.bridge.weak.upgrade_in_event_loop(move |app| {
            if locked.load(Ordering::Acquire) { return; }
            let st = app.global::<AppState>();
            st.set_busy(true);
            st.set_busy_text(title);
        });
        std::thread::spawn(move || {
            *ctx.bridge.pin_cache.lock().unwrap() = None;
            // Persist a start marker so an interrupted session cannot look successful.
            let result = ctx.save().and_then(|()|f(&ctx));
            let status=crate::history::Status::result(result.is_ok(),ctx.bridge.cancelled.load(Ordering::Acquire));
            ctx.vault.lock().unwrap().history.finish(activity,status);
            *ctx.bridge.pin_cache.lock().unwrap() = None;
            let save = ctx.save();
            if save.is_err() { ctx.vault.lock().unwrap().history.save_failed(activity); }
            // Keep the session reserved until its final UI update has run, so a
            // vault switch cannot overtake callbacks belonging to the old vault.
            let running = ctx.running.clone();
            let cancelled = ctx.bridge.cancelled.clone();
            let locked = ctx.bridge.locked.clone();
            let queued = ctx.bridge.weak.upgrade_in_event_loop(move |app| {
                let st = app.global::<AppState>();
                st.set_busy(false);
                running.store(false, Ordering::Release);
                crate::notifications::clear_reader();
                if notify_sync {
                    crate::notifications::sync_finished(if cancelled.load(Ordering::Acquire) {crate::notifications::Outcome::Cancelled}
                        else if result.is_ok() && save.is_ok() {crate::notifications::Outcome::Success}
                        else {crate::notifications::Outcome::Error});
                }
                if locked.load(Ordering::Acquire) {
                    crate::session::finish_lock(&app);
                    if save.is_err() { st.set_unlock_error("Tresor gesperrt. Letztes Speichern fehlgeschlagen; Bankstatus vor erneutem Senden prüfen.".into()); }
                    return;
                }
                st.set_show_pinpad(false);
                crate::refresh_view(&app);
                let result=if st.get_tan_kind()==4 && cancelled.load(Ordering::Acquire) {
                    Err("Statusprüfung beendet. Bitte den Bankstatus prüfen, bevor du den Auftrag erneut sendest.".into())
                } else { result };
                let (message,error)=match (result, save) {
                    (Ok(msg), Ok(())) => (msg,false),
                    (Ok(msg), Err(e)) => (i18n::tr_format!("{msg}\n\nLokales Speichern fehlgeschlagen: {e}"),true),
                    (Err(e), Ok(())) => (e,true),
                    (Err(e), Err(save)) => (i18n::tr_format!("{e}\n\nLokales Speichern fehlgeschlagen: {save}"),true),
                };
                if st.get_tan_kind()==4 {
                    st.set_tan_kind(if error {6} else {5});
                    st.set_tan_title(if error {"Ergebnis der Freigabeprüfung"} else {"Bankanfrage abgeschlossen"}.into());
                    st.set_tan_text(message); st.set_tan_status(Default::default());
                    st.set_show_tan(true);
                } else { crate::toast(&app,&message,error); }
            });
            if queued.is_err() { ctx.running.store(false, Ordering::Release); }
        });
    }
}

/// Persist native state and translate errors for the current UI session.
fn conclude<T>(ctx: &Ctx, access_id: u32, client: impl RefreshClient, r: Result<T, Error>) -> Result<T, String> {
    let state = client.finish();
    store_state(ctx, access_id, state);
    let wrong_pin = matches!(&r, Err(Error::Bank { code, text } | Error::Access { code, text })
        if ["9931", "9942", "9340", "9930"].iter().any(|c| code == c || text.contains(c)));
    let pin_note = handle_saved_pin(ctx, access_id, wrong_pin);
    let r = r.inspect(|_| {
        if let Some(n) = &pin_note {
            log::info!("{n}");
        }
    });
    r.map_err(|e| err_text(ctx, &e))
}

/// Store the PIN when the user asked for it, drop a stored PIN the bank rejected.
fn handle_saved_pin(ctx: &Ctx, access_id: u32, wrong_pin: bool) -> Option<String> {
    let bridge = &ctx.bridge;
    let rejected=bridge.pin_was_rejected.swap(false,Ordering::AcqRel);
    let mut v = ctx.vault.lock().unwrap();
    let access = v.accesses.iter_mut().find(|a| a.id == access_id)?;
    if wrong_pin || rejected {
        *bridge.pin_cache.lock().unwrap() = None;
        *bridge.pin_save_requested.lock().unwrap() = false;
        if access.saved_pin.take().is_some() {
            v.history.add("Abgelehnte gespeicherte PIN gelöscht",crate::history::Status::Success,None);
            return Some(i18n::tr("Die gespeicherte PIN wurde von der Bank abgelehnt und gelöscht.").into());
        }
        return None;
    }
    if std::mem::take(&mut *bridge.pin_save_requested.lock().unwrap()) {
        let pin = bridge.pin_cache.lock().unwrap().clone()?;
        let sealed = ctx.file.lock().unwrap().as_ref()?.seal_secret(&pin).ok()?;
        access.saved_pin = Some(sealed);
        v.history.add("PIN verschlüsselt gespeichert",crate::history::Status::Success,None);
        return Some(i18n::tr("PIN verschlüsselt gespeichert.").into());
    }
    None
}

fn err_text(ctx: &Ctx, e: &Error) -> String {
    match e {
        Error::Cancelled => {
            ctx.bridge.cancelled.store(true, Ordering::Release);
            i18n::tr("Vorgang abgebrochen.").into()
        },
        Error::Bank { text, .. } => i18n::tr_format!("Die Bank meldet:\n{text}"),
        Error::Transport(t) => i18n::tr_format!("Verbindung zur Bank fehlgeschlagen: {t}"),
        Error::NotSupported(t) => i18n::tr_format!("Von der Bank nicht angeboten: {t}"),
        other => other.to_string(),
    }
}

fn medium_for(_ctx: &Ctx, access: &BankAccess) -> Result<Medium, String> {
    match &access.kind {
        AccessKind::PinTan => Ok(Medium::PinTan),
        AccessKind::ChipCard { reader, record } => Ok(Medium::Ddv { reader: reader.clone(), record: *record }),
        AccessKind::RdhCard { reader, record } => Ok(Medium::Rdh { reader: reader.clone(), record: *record }),
        AccessKind::Demo => Err(i18n::tr("Demo-Zugang").into()),
        AccessKind::PayPal { .. } => Err(crate::paypal::read_only_error()),
    }
}

pub fn read_rdh_card(ctx: &Ctx, reader: &str, record: u8) -> Result<(aqbanking::CardContext, String), String> {
    aqbanking::read_card(Medium::Rdh { reader: reader.into(), record }, &ctx.bridge).map_err(|e| err_text(ctx,&e))
}

fn make_client<'a>(ctx: &Ctx, access: &BankAccess, bridge: &'a Bridge) -> Result<Client<'a>, String> {
    bridge.pin_was_rejected.store(false,Ordering::Release);
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
    let cfg = access.config.clone();
    Client::new(cfg, access.state.clone(), medium, bridge).map_err(|e| err_text(ctx, &e))
}

fn store_state(ctx: &Ctx, access_id: u32, state: banking::State) {
    let mut v = ctx.vault.lock().unwrap();
    if let Some(a) = v.accesses.iter_mut().find(|a| a.id == access_id) {
        a.state = state;
    }
}

fn allowed(acc: &Account, codes: &[&str]) -> bool {
    acc.allowed.is_empty() || codes.iter().any(|c| acc.allowed.iter().any(|a| a == c))
}

/// Send this access's saved orders first, then refresh accounts, balances and transactions.
pub fn sync_access(ctx: &Ctx, access_id: u32) -> JobResult {
    ctx.vault.lock().unwrap().access(access_id).ok_or(i18n::tr("Bankzugang nicht gefunden"))?;
    sync_targets(ctx, vec![SyncTarget::access(access_id)])
}

/// Synchronize saved orders, balances and transactions only for the selected account.
pub fn sync_account(ctx: &Ctx, account_id: u32) -> JobResult {
    let (account, access) = account_and_access(ctx, account_id)?;
    sync_targets(ctx, vec![SyncTarget { access_id: access.id, accounts: Some(vec![(account, account_id)]) }])
}

type AccountBatch = Vec<(Account, u32)>;
struct SyncTarget { access_id: u32, accounts: Option<AccountBatch> }
impl SyncTarget {
    fn access(access_id: u32) -> Self { Self { access_id, accounts: None } }
}

fn group_targets(ctx: &Ctx, group_id: u32) -> Result<Vec<SyncTarget>, String> {
    let ids = {
        let v = ctx.vault.lock().unwrap();
        let group = if group_id == 0 { None } else {
            Some(v.groups.iter().find(|g| g.id == group_id).ok_or(i18n::tr("Gruppe nicht gefunden"))?)
        };
        v.accounts.iter().filter(|a| match group {
            Some(g) => g.account_ids.contains(&a.id),
            None => !v.groups.iter().any(|g| g.account_ids.contains(&a.id)),
        }).map(|a| a.id).collect::<Vec<_>>()
    };
    if ids.is_empty() { return Err(i18n::tr("Keine Konten").into()); }
    let mut targets: Vec<SyncTarget> = Vec::new();
    for id in ids {
        let (account, access) = account_and_access(ctx, id)?;
        if let Some(target) = targets.iter_mut().find(|target| target.access_id == access.id) {
            target.accounts.as_mut().expect("group accounts are selected").push((account, id));
        } else {
            targets.push(SyncTarget { access_id: access.id, accounts: Some(vec![(account, id)]) });
        }
    }
    Ok(targets)
}

/// Send the group's saved orders before refreshing only its accounts.
pub fn sync_group(ctx: &Ctx, group_id: u32) -> JobResult {
    sync_targets(ctx, group_targets(ctx, group_id)?)
}

fn sync_targets(ctx: &Ctx, targets: Vec<SyncTarget>) -> JobResult {
    sync_targets_with(ctx, targets, perform_transfer, sync_scope)
}

fn sync_targets_with(
    ctx: &Ctx, targets: Vec<SyncTarget>,
    send: impl FnMut(&Ctx, u32, SepaTransfer, bool) -> JobResult,
    mut fetch: impl FnMut(&Ctx, BankAccess, Option<AccountBatch>) -> JobResult,
) -> JobResult {
    // Complete the entire outgoing phase before opening any retrieval dialog,
    // including when a group or the global synchronization spans several banks.
    let sent = dispatch_queued_with(ctx, &targets, send)?;
    let mut messages = Vec::new();
    let mut failed = false;
    for target in targets {
        if ctx.bridge.cancelled.load(Ordering::Acquire) { return Err(i18n::tr("Vorgang abgebrochen.").into()); }
        // Sending can update TAN/dialog state. Never reuse the pre-send snapshot.
        let access = ctx.vault.lock().unwrap().access(target.access_id).cloned().ok_or(i18n::tr("Bankzugang nicht gefunden"))?;
        match fetch(ctx, access, target.accounts) {
            Ok(message) => messages.push(message),
            Err(error) if ctx.bridge.cancelled.load(Ordering::Acquire) => return Err(error),
            Err(error) => { failed = true; messages.push(error); },
        }
    }
    if sent > 0 { messages.push(i18n::tr_format!("\n{} gespeicherte Überweisungen gesendet.", sent).trim_start().into()); }
    if failed { Err(messages.join("\n")) } else { Ok(messages.join("\n")) }
}

fn scope_name(ctx: &Ctx, access: &BankAccess, selected: Option<&AccountBatch>) -> String {
    match selected {
        Some(accounts) if accounts.len() == 1 => view::account_label(&ctx.vault.lock().unwrap(), accounts[0].1),
        _ => access.name.clone(),
    }
}

fn sync_scope(ctx: &Ctx, access: BankAccess, selected: Option<AccountBatch>) -> JobResult {
    if matches!(access.kind, AccessKind::PayPal { .. }) {
        return crate::paypal::sync(ctx, &access, selected).map_err(|e| format!("{}:\n{e}", access.name));
    }
    if access.kind == AccessKind::Demo {
        let name = scope_name(ctx, &access, selected.as_ref());
        return Ok(i18n::tr_format!("{}: Demo-Daten sind aktuell.", name));
    }
    let bridge = ctx.bridge.with_application_progress();
    let client = make_client(ctx, &access, &bridge).map_err(|e| format!("{}:\n{e}", access.name))?;
    sync_with_client(ctx, &access, selected, client)
}

fn sync_with_client(ctx: &Ctx, access: &BankAccess, selected: Option<AccountBatch>, mut client: impl RefreshClient) -> JobResult {
    let access_id = access.id;
    let single_account = selected.as_ref().is_some_and(|accounts| accounts.len() == 1);
    let name = scope_name(ctx, access, selected.as_ref());
    let mut warnings: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut new_tx = 0usize;
    let result: Result<usize, Error> = (|| {
        ctx.bridge.status(&i18n::tr_format!("{}: Kontodaten aktualisieren …", name));
        let (accounts, ids): (Vec<_>, Vec<_>) = if let Some(accounts) = selected {
            let (old,ids): (Vec<_>, Vec<_>) = accounts.into_iter().unzip();
            let mut accounts = client.current_accounts(old)?;
            if accounts.len()!=ids.len() { return Err(Error::Security("Unvollständige AqBanking-Kontozuordnung".into())); }
            let mut vault=ctx.vault.lock().unwrap();
            for (account,id) in accounts.iter_mut().zip(&ids) {
                crate::banks::complete_account(account);
                if let Some(stored)=vault.accounts.iter_mut().find(|a| a.id==*id && a.access_id==access_id) {
                    stored.account=account.clone();
                }
            }
            (accounts,ids)
        } else {
            let mut accounts = client.accounts()?;
            for account in &mut accounts { crate::banks::complete_account(account); }
            let ids = ctx.vault.lock().unwrap().merge_accounts(access_id, accounts.clone());
            (accounts, ids)
        };
        let dates={
            let vault=ctx.vault.lock().unwrap();
            ids.iter().map(|id|vault.last_booking(*id).map(|d|d-Duration::days(10))
                .unwrap_or_else(||Local::now().date_naive()-Duration::days(89))).collect::<Vec<_>>()
        };
        let reads=accounts.iter().zip(&dates).filter(|(a,_)|!(30..=39).contains(&a.kind))
            .map(|(a,from)|aqbanking::ReadRequest {account:a.clone(),from:*from,
                balance:allowed(a,&["HKSAL","HKKKS","DKKKS"]),transactions:allowed(a,&["HKKAZ","HKCAZ","HKKKU","DKKKU"])})
            .filter(|r|r.balance || r.transactions).collect();
        let batched=client.prepare_reads(reads)?;
        let mut updated=0;
        for ((acc, id), from) in accounts.iter().zip(ids).zip(dates) {
            let warnings_before = warnings.len();
            let mut fetched = false;
            let mut unavailable = false;
            if (30..=39).contains(&acc.kind) {
                continue; // securities accounts are out of scope
            }
            let label = view::account_label(&ctx.vault.lock().unwrap(), id);
            if allowed(acc, &["HKSAL", "HKKKS", "DKKKS"]) {
                ctx.bridge.status(&if batched {i18n::tr_format!("{}: Salden und Umsätze abrufen …",name)} else {i18n::tr_format!("{}: Saldo abrufen …", label)});
                match client.balance(acc) {
                    Ok(Some(b)) => {
                        fetched = true;
                        let mut v = ctx.vault.lock().unwrap();
                        if let Some(s) = v.accounts.iter_mut().find(|s| s.id == id) {
                            s.balance = Some(b);
                        }
                    }
                    Ok(None) | Err(Error::NotSupported(_)) => {}
                    Err(e) if stops_refresh(&e) => return Err(e),
                    Err(e) => warnings.push(i18n::tr_format!("Saldo {}: {}", banking::display::display_account(acc), err_text(ctx, &e))),
                }
            }
            if allowed(acc, &["HKKAZ", "HKCAZ", "HKKKU", "DKKKU"]) {
                ctx.bridge.status(&if batched {i18n::tr_format!("{}: Salden und Umsätze abrufen …",name)} else {i18n::tr_format!("{}: Umsätze abrufen …", label)});
                match client.transaction_report(acc, from, None) {
                    Ok(report) => {
                        fetched = true;
                        warnings.extend(report.warnings);
                        let mut v = ctx.vault.lock().unwrap();
                        if let Some(balance) = report.balance {
                            if let Some(stored) = v.accounts.iter_mut().find(|s| s.id == id) { stored.balance = Some(balance); }
                        }
                        new_tx += v.merge_aqbanking_transactions(id, report.transactions);
                    }
                    Err(Error::NotSupported(e)) => {
                        unavailable = true;
                        notes.push(i18n::tr_format!("Umsätze {}: {}", banking::display::display_account(acc), e));
                    }
                    Err(e) if stops_refresh(&e) => return Err(e),
                    Err(e) => {
                        let message = i18n::tr_format!("Umsätze {}: {}", banking::display::display_account(acc), err_text(ctx, &e));
                        warnings.push(message);
                    },
                }
            } else {
                notes.push(i18n::tr_format!("Umsätze {}: In den aktuellen Kontodaten ist kein unterstützter Umsatzabruf verfügbar.", banking::display::display_account(acc)));
            }
            if fetched { updated+=1; }
            let mut v = ctx.vault.lock().unwrap();
            if let Some(s) = v.accounts.iter_mut().find(|s| s.id == id) {
                if fetched && !unavailable && warnings.len() == warnings_before { s.last_sync = Some(Local::now().naive_local()); }
            }
        }
        Ok(updated)
    })();
    let count = conclude(ctx, access_id, client, result).map_err(|e| format!("{name}:\n{e}"))?;
    let mut msg = if single_account && count==0 {
        i18n::tr_format!("{}: Keine Kontodaten aktualisiert.", name)
    } else if single_account {
        i18n::tr_format!("{}: Konto aktualisiert, {} neue Umsätze.", name, new_tx)
    } else {
        i18n::tr_format!("{}: {} Konten aktualisiert, {} neue Umsätze.", name, count, new_tx)
    };
    if !notes.is_empty() { msg.push('\n'); msg.push_str(&notes.join("\n")); }
    if !warnings.is_empty() {
        msg.push('\n');
        msg.push_str(&warnings.join("\n"));
    }
    if warnings.is_empty() { Ok(msg) } else { Err(msg) }
}

// A failed access setup or transport must not be retried for every account and
// operation. Preserve partial results/state, then let the user resolve the cause.
fn stops_refresh(error: &Error) -> bool {
    match error {
        Error::Access { .. } | Error::Transport(_) | Error::Security(_) | Error::Cancelled => true,
        Error::Bank { code, .. } => matches!(code.as_str(), "9930" | "9931" | "9942" | "9340"),
        _ => false,
    }
}

pub fn sync_all(ctx: &Ctx) -> JobResult {
    let ids: Vec<u32> = ctx.vault.lock().unwrap().accesses.iter().map(|a| a.id).collect();
    if ids.is_empty() {
        return Err(i18n::tr("Noch kein Bankzugang eingerichtet.").into());
    }
    sync_targets(ctx, ids.into_iter().map(SyncTarget::access).collect())
}

pub fn reset_tan(ctx: &Ctx, access_id: u32) -> JobResult {
    let access = ctx.vault.lock().unwrap().access(access_id).cloned().ok_or(i18n::tr("Bankzugang nicht gefunden"))?;
    let bridge = ctx.bridge.clone();
    let mut client = make_client(ctx, &access, &bridge)?;
    let r = client.reset_tan();
    conclude(ctx, access_id, client, r)?;
    Ok("TAN-Verfahren wurde in AqBanking gespeichert.".into())
}

fn account_and_access(ctx: &Ctx, account_id: u32) -> Result<(Account, BankAccess), String> {
    let v = ctx.vault.lock().unwrap();
    let a = v.account(account_id).ok_or(i18n::tr("Konto nicht gefunden"))?;
    let access = v.access(a.access_id).cloned().ok_or(i18n::tr("Bankzugang nicht gefunden"))?;
    let mut acc = a.account.clone();
    crate::banks::complete_account(&mut acc);
    if acc.blz.is_empty() {
        acc.blz = access.config.blz.clone();
    }
    Ok((acc, access))
}

fn demo_tan(ctx: &Ctx, what: &str) -> Result<(), String> {
    use banking::Interaction;
    let ch = TanChallenge {
        text: i18n::tr_format!("Demo: {what}\n\nIm Demo-Modus wird nichts an eine Bank gesendet. Geben Sie eine beliebige TAN ein."),
        method: TanMethod {
            name: i18n::tr("chipTAN optisch (Demo)").into(),
            technical_id: "HHD1.4OPT".into(),
            decoupled: false,
            max_tan_length: 6,
        },
        decoupled: false,
    };
    match ctx.bridge.tan(&ch, Some(demo::DEMO_FLICKER), None) {
        banking::TanAnswer::Cancel => Err(err_text(ctx, &Error::Cancelled)),
        _ => Ok(()),
    }
}

/// Immediate sending uses the same durable record as a saved order.
pub fn transfer(ctx: &Ctx, account_id: u32, t: SepaTransfer, scheduled: bool, save_recipient: bool) -> JobResult {
    let id = ctx.edit_vault_logged("Überweisung gespeichert", |v| v.queue_transfer(account_id, t, scheduled, save_recipient))?;
    submit_order_with(ctx, id, perform_transfer)
}

fn dispatch_queued_with(
    ctx: &Ctx, targets: &[SyncTarget],
    mut send: impl FnMut(&Ctx, u32, SepaTransfer, bool) -> JobResult,
) -> Result<usize, String> {
    let ids: Vec<_> = {
        let v = ctx.vault.lock().unwrap();
        v.payment_orders.iter().filter(|order| order.status == OrderStatus::Queued
            && v.account(order.account_id).is_some_and(|account| targets.iter().any(|target|
                target.access_id == account.access_id && target.accounts.as_ref().is_none_or(|accounts|
                    accounts.iter().any(|(_, id)| *id == account.id)))))
            .map(|order| order.id).collect()
    };
    let mut sent = 0;
    for id in ids {
        if ctx.bridge.cancelled.load(Ordering::Acquire) { return Err(i18n::tr("Vorgang abgebrochen.").into()); }
        submit_order_with(ctx, id, &mut send)?;
        sent += 1;
    }
    Ok(sent)
}

fn submit_order_with(
    ctx: &Ctx, id: u32,
    send: impl FnOnce(&Ctx, u32, SepaTransfer, bool) -> JobResult,
) -> JobResult {
    if ctx.bridge.cancelled.load(Ordering::Acquire) { return Err(i18n::tr("Vorgang abgebrochen.").into()); }
    // If persistence fails, no bank request is made. A crash or lost reply leaves a
    // non-queued record on disk, so a later synchronization cannot send it twice.
    let (order, activity): (PaymentOrder, u64) = ctx.edit_vault(|v| {
        let order = v.payment_orders.iter_mut().find(|o| o.id == id).ok_or(i18n::tr("Auftrag nicht gefunden"))?;
        if order.status != OrderStatus::Queued { return Err(i18n::tr("Dieser Auftrag wurde bereits verarbeitet. Bitte den Status prüfen.").into()); }
        order.status = OrderStatus::Submitting;
        let order=order.clone();
        let transfer=crate::history::Transfer::from_order(v,&order);
        let activity=v.history.add("Überweisung eingereicht",crate::history::Status::Running,Some(transfer));
        Ok((order,activity))
    })?;
    let result = (|| {
        let t = ctx.vault.lock().unwrap().prepare_transfer(order.account_id, order.transfer, order.scheduled)?;
        send(ctx, order.account_id, t, order.scheduled)
    })();
    {
        let mut v = ctx.vault.lock().unwrap();
        let stored = v.payment_orders.iter_mut().find(|o| o.id == id).expect("order retained while job runs");
        stored.status = if result.is_ok() { OrderStatus::Sent } else { OrderStatus::NeedsReview };
        stored.message = match &result { Ok(message) | Err(message) => message.clone() };
        v.history.finish(activity,if result.is_ok() {crate::history::Status::Success} else {crate::history::Status::NeedsReview});
    }
    // Keep the in-memory outcome even on a save failure. Disk still contains the
    // Submitting marker, which is deliberately never eligible for automatic retry.
    if let Err(e) = ctx.save() {
        ctx.vault.lock().unwrap().history.save_failed(activity);
        return Err(i18n::tr_format!("Auftragsstatus konnte nicht gespeichert werden: {e}. Bitte vor einem erneuten Auftrag den Bankstatus prüfen."));
    }
    result
}

fn perform_transfer(ctx: &Ctx, account_id: u32, t: SepaTransfer, scheduled: bool) -> JobResult {
    let (acc, access) = account_and_access(ctx, account_id)?;
    let mut t = t;
    t.debtor_iban = acc.iban.clone();
    t.debtor_bic = acc.bic.clone();
    if t.debtor_name.is_empty() {
        t.debtor_name = if acc.owner.is_empty() { i18n::tr("Kontoinhaber").into() } else { acc.owner.clone() };
    }
    confirm_payment(ctx, &t, if scheduled { "Terminüberweisung freigeben" } else { "Überweisung freigeben" })?;
    if access.kind == AccessKind::Demo {
        let result = banking::payee::PayeeVerification { status: banking::payee::PayeeStatus::Unavailable,
            name: t.creditor_name.clone(), iban: t.creditor_iban.clone(), suggested_name: String::new(),
            reason: i18n::tr("Demo-Modus: Es findet keine Empfängerprüfung durch eine Bank statt.").into(), bank_notice: String::new() };
        if !banking::Interaction::verify_payee(&ctx.bridge, &result, false) { return Err(err_text(ctx, &Error::Cancelled)); }
        demo_tan(ctx, &i18n::tr_format!("Überweisung {} an {}", view::fmt_eur(t.amount), t.creditor_name))?;
        let mut tx = banking::Transaction {
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
        return Ok(i18n::tr("Demo: Überweisung wurde simuliert und als vorgemerkter Umsatz eingetragen.").into());
    }
    let bridge = ctx.bridge.clone();
    let mut client = make_client(ctx, &access, &bridge)?;
    let r = if scheduled { client.scheduled_transfer(&acc, &t) } else { client.transfer(&acc, &t) };
    let summary = conclude(ctx, access.id, client, r)?;
    Ok(i18n::tr_format!("Auftrag übermittelt.\n{summary}"))
}

fn confirm_payment(ctx: &Ctx, transfer: &SepaTransfer, action: &str) -> Result<(), String> {
    use banking::Interaction;
    // Bank/user strings must not insert fake field labels or reverse IBAN
    // display with bidi controls. Escaping is presentation-only.
    fn visible(s:&str)->String {
        s.chars().flat_map(|c| {
            if c.is_control() || matches!(c,'\u{200e}'|'\u{200f}'|'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}') {
                c.escape_unicode().to_string().chars().collect::<Vec<_>>()
            } else {vec![c]}
        }).collect()
    }
    let details = format!("Auftraggeber: {}\nVon IBAN: {}\n\nEmpfänger: {}\nAn IBAN: {}\nBIC: {}\nBetrag: {}\nVerwendungszweck: {}\nAusführung: {}{}\n\nBitte alle Angaben vor der Freigabe prüfen.",
        visible(&transfer.debtor_name), visible(&transfer.debtor_iban), visible(&transfer.creditor_name), visible(&transfer.creditor_iban), visible(&transfer.creditor_bic),
        view::fmt_eur(transfer.amount), visible(&transfer.purpose),
        transfer.execution_date.map(|d| d.to_string()).unwrap_or_else(|| "sobald möglich".into()),
        if transfer.instant { " (Echtzeit)" } else { "" });
    if ctx.bridge.cancelled() || !ctx.bridge.confirm(action, &details) || ctx.bridge.cancelled() {
        return Err(err_text(ctx, &Error::Cancelled));
    }
    Ok(())
}

pub fn fetch_standing(ctx: &Ctx) -> JobResult {
    let groups: Vec<(BankAccess, Vec<(u32, Account)>)> = {
        let v = ctx.vault.lock().unwrap();
        v.accesses
            .iter()
            .filter(|a| !matches!(a.kind, AccessKind::Demo | AccessKind::PayPal { .. }))
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
        return Ok(i18n::tr("Nur Demo-Daueraufträge vorhanden.").into());
    }
    let mut total = 0;
    let mut loaded_accounts = 0;
    let mut warnings = Vec::new();
    for (access, accs) in groups {
        if accs.is_empty() {
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
                    return Err(i18n::tr("Vorgang abgebrochen.").into());
                }
                Err(e) => {
                    wrong_pin = matches!(&e, Error::Bank { code, text } | Error::Access { code, text }
                        if ["9931", "9942", "9340", "9930"].iter().any(|c| code == c || text.contains(c)));
                    let msg = format!("{} · {}: {}", access.name, acc.product, err_text(ctx, &e));
                    warnings.push(msg);
                    if wrong_pin || stops_refresh(&e) { break; }
                },
            }
        }
        let st = client.finish();
        store_state(ctx, access.id, st);
        handle_saved_pin(ctx, access.id, wrong_pin);
    }
    if !warnings.is_empty() {
        let status = if loaded_accounts == 0 {
            i18n::tr("Daueraufträge konnten nicht abgerufen werden.").to_string()
        } else {
            i18n::tr_format!("Abruf teilweise erfolgreich: {total} Daueraufträge aus {loaded_accounts} Konten geladen.")
        };
        return Err(i18n::tr_format!("{status}\n{}\nVorhandene Daueraufträge der betroffenen Konten bleiben erhalten.", warnings.join("\n")));
    }
    if loaded_accounts == 0 {
        return Ok(i18n::tr("Keines der eingerichteten Konten bietet den Dauerauftragsabruf an.").into());
    }
    Ok(i18n::tr_format!("{total} Daueraufträge geladen."))
}

pub fn create_standing(ctx: &Ctx, account_id: u32, mut so: StandingOrder) -> JobResult {
    let (acc, access) = account_and_access(ctx, account_id)?;
    so.transfer.debtor_iban = acc.iban.clone();
    so.transfer.debtor_bic = acc.bic.clone();
    if so.transfer.debtor_name.is_empty() {
        so.transfer.debtor_name = if acc.owner.is_empty() { i18n::tr("Kontoinhaber").into() } else { acc.owner.clone() };
    }
    let details = format!("Dauerauftrag: {:?}, alle {}, Ausführungstag {}, Beginn {}, Ende {}",
        so.interval, so.every, so.day, so.first_date, so.last_date.map(|d| d.to_string()).unwrap_or_else(|| "unbefristet".into()));
    confirm_payment(ctx, &so.transfer, &details)?;
    if access.kind == AccessKind::Demo {
        demo_tan(ctx, &i18n::tr_format!("Dauerauftrag {} an {}", view::fmt_eur(so.transfer.amount), so.transfer.creditor_name))?;
        so.order_id = format!("DEMO-{}", ctx.vault.lock().unwrap().new_id());
        ctx.vault.lock().unwrap().standing.push(StoredStanding { account_id, order: so });
        return Ok(i18n::tr("Demo: Dauerauftrag angelegt.").into());
    }
    let bridge = ctx.bridge.clone();
    let mut client = make_client(ctx, &access, &bridge)?;
    let r = client.create_standing_order(&acc, &so);
    // Failure to refresh the list must not make an accepted order look unsuccessful.
    let r2 = r.map(|s| (s, client.standing_orders(&acc)));
    let (summary, list) = conclude(ctx, access.id, client, r2)?;
    let list = match list {
        Ok(list) => list,
        Err(e) => return Ok(i18n::tr_format!("Dauerauftrag angelegt.\n{summary}\n\nDie Liste konnte nicht aktualisiert werden: {}. Bitte die Liste erneut abrufen, den Auftrag nicht erneut anlegen.", err_text(ctx, &e))),
    };
    let mut v = ctx.vault.lock().unwrap();
    v.standing.retain(|s| s.account_id != account_id);
    v.standing.extend(list.into_iter().map(|order| StoredStanding { account_id, order }));
    Ok(i18n::tr_format!("Dauerauftrag angelegt.\n{summary}"))
}

pub fn delete_standing(ctx: &Ctx, index: usize) -> JobResult {
    let item = ctx.vault.lock().unwrap().standing.get(index).cloned().ok_or(i18n::tr("Dauerauftrag nicht gefunden"))?;
    let (acc, access) = account_and_access(ctx, item.account_id)?;
    confirm_payment(ctx, &item.order.transfer, "Dauerauftrag löschen")?;
    if access.kind != AccessKind::Demo {
        let bridge = ctx.bridge.clone();
        let mut client = make_client(ctx, &access, &bridge)?;
        let r = client.delete_standing_order(&acc, &item.order);
        conclude(ctx, access.id, client, r)?;
    } else {
        demo_tan(ctx, &i18n::tr_format!("Dauerauftrag an {} löschen", item.order.transfer.creditor_name))?;
    }
    let mut v = ctx.vault.lock().unwrap();
    v.standing.retain(|s| !(s.account_id == item.account_id && s.order.order_id == item.order.order_id));
    Ok(i18n::tr("Dauerauftrag gelöscht.").into())
}

pub fn read_card(ctx: &Ctx, reader: &str, record: u8) -> Result<aqbanking::CardContext, String> {
    aqbanking::read_card(Medium::Ddv {reader:reader.into(),record}, &ctx.bridge).map(|(card,_)| card).map_err(|e| err_text(ctx,&e))
}

#[cfg(test)]
mod business_tests {
    use super::*;
    fn context() -> Ctx {
        Ctx { vault: Arc::new(Mutex::new(Vault::default())), file: Arc::new(Mutex::new(None)),
            bridge: Bridge::new(crate::ui::Weak::default()), running: Arc::new(AtomicBool::new(false)) }
    }
    #[test]
    fn cancellation_control_flow_does_not_depend_on_translated_or_bank_supplied_text() {
        for language in [i18n::Language::German, i18n::Language::English, i18n::Language::French, i18n::Language::Spanish] {
            i18n::with_language(language, || {
                let ctx = context();
                let bank = Error::Bank { code: "9000".into(), text: "abgebrochen / cancelled".into() };
                assert!(err_text(&ctx, &bank).contains("abgebrochen / cancelled"));
                assert!(!ctx.bridge.cancelled.load(Ordering::Acquire));
                assert_eq!(err_text(&ctx, &Error::Cancelled), i18n::tr("Vorgang abgebrochen."));
                assert!(ctx.bridge.cancelled.load(Ordering::Acquire));
            });
        }
    }
    #[test]
    fn demo_setup_and_refresh_are_idempotent_and_never_contact_a_bank() {
        let ctx = context(); demo::add_demo(&mut ctx.vault.lock().unwrap());
        let before = serde_json::to_vec(&*ctx.vault.lock().unwrap()).unwrap();
        demo::add_demo(&mut ctx.vault.lock().unwrap()); assert!(sync_all(&ctx).unwrap().contains("Demo"));
        assert!(fetch_standing(&ctx).unwrap().contains("Demo"));
        assert_eq!(serde_json::to_vec(&*ctx.vault.lock().unwrap()).unwrap(), before);
        assert!(ctx.vault.lock().unwrap().accounts.iter().all(|a| banking::validation::iban_valid(&a.account.iban)));
    }

    #[test]
    fn combined_sync_names_the_failed_access_after_another_access_succeeds() {
        let ctx = context();
        demo::add_demo(&mut ctx.vault.lock().unwrap());
        {
            let mut vault = ctx.vault.lock().unwrap();
            let mut access = vault.accesses[0].clone();
            access.id = vault.new_id();
            access.name = "VR Testbank".into();
            access.kind = AccessKind::PinTan;
            access.config.url = "http://bank.invalid".into(); // fails before any network/PIN request
            vault.accesses.push(access);
        }
        let before = serde_json::to_vec(&*ctx.vault.lock().unwrap()).unwrap();
        let message = sync_all(&ctx).unwrap_err();
        assert!(message.contains("Demo-Daten sind aktuell."));
        assert!(message.contains("\nVR Testbank:\nFinTS benötigt"), "{message}");
        assert_eq!(serde_json::to_vec(&*ctx.vault.lock().unwrap()).unwrap(), before);
    }
    #[test]
    fn missing_access_account_or_order_fails_before_prompting_or_mutating() {
        let ctx = context(); assert!(sync_all(&ctx).is_err()); assert!(sync_access(&ctx, 999).is_err());
        assert!(reset_tan(&ctx, 999).is_err()); assert!(delete_standing(&ctx, 999).is_err());
        let so = crate::business_tests::standing(); assert!(transfer(&ctx, 999, so.transfer.clone(), false, false).is_err());
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

#[cfg(test)]
mod outgoing_order_tests {
    use super::*;
    struct Fixture { ctx: Ctx, dir: std::path::PathBuf, path: std::path::PathBuf, account: u32, access: u32 }
    impl Fixture {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("bankingcraft-orders-{:016x}", rand::random::<u64>()));
            std::fs::create_dir(&dir).unwrap();
            let path = dir.join("test.vault");
            let mut vault = Vault::default(); demo::add_demo(&mut vault);
            let account = vault.accounts[0].id; let access = vault.accounts[0].access_id;
            let ctx = Ctx { vault: Arc::new(Mutex::new(vault)),
                file: Arc::new(Mutex::new(Some(VaultFile::create(path.clone(), "test").unwrap()))),
                bridge: Bridge::new(crate::ui::Weak::default()), running: Arc::new(AtomicBool::new(false)) };
            ctx.save().unwrap();
            Self { ctx, dir, path, account, access }
        }
        fn queue(&self) -> u32 {
            self.ctx.edit_vault(|v| v.queue_transfer(self.account, crate::business_tests::standing().transfer, false, false)).unwrap()
        }
        fn reload(&self) -> Vault { VaultFile::open(self.path.clone(), "test").unwrap().1 }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.dir); } }

    #[test]
    fn global_sync_sends_all_orders_before_any_retrieval_and_uses_updated_bank_state() {
        let f = Fixture::new();
        f.queue();
        let other_access = f.ctx.edit_vault(|v| {
            let mut access = v.accesses[0].clone(); access.id = v.new_id();
            let mut account = v.accounts[0].clone(); account.id = v.new_id(); account.access_id = access.id;
            let access_id = access.id; let account_id = account.id;
            v.accesses.push(access); v.accounts.push(account);
            v.queue_transfer(account_id, crate::business_tests::standing().transfer, false, false)?;
            Ok(access_id)
        }).unwrap();
        let events = std::cell::RefCell::new(Vec::new());
        let result = sync_targets_with(&f.ctx, vec![SyncTarget::access(f.access), SyncTarget::access(other_access)],
            |ctx, account, _, _| {
                let mut v = ctx.vault.lock().unwrap();
                let access_id = v.account(account).unwrap().access_id;
                events.borrow_mut().push(format!("send:{access_id}"));
                v.accesses.iter_mut().find(|a| a.id == access_id).unwrap().state.aqbanking.get_or_insert_with(Default::default).binding = "AFTER_PAYMENT".into();
                Ok("Accepted".into())
            },
            |ctx, access, selected| {
                assert!(selected.is_none());
                assert_eq!(access.state.aqbanking.as_ref().unwrap().binding, "AFTER_PAYMENT");
                assert!(ctx.vault.lock().unwrap().payment_orders.iter().all(|o| o.status == OrderStatus::Sent));
                events.borrow_mut().push(format!("fetch:{}", access.id));
                Ok("Retrieved".into())
            }).unwrap();
        assert!(result.contains("Retrieved"));
        assert_eq!(*events.borrow(), [format!("send:{}", f.access), format!("send:{other_access}"),
            format!("fetch:{}", f.access), format!("fetch:{other_access}")]);
        *f.ctx.vault.lock().unwrap() = f.reload();
        sync_targets_with(&f.ctx, vec![SyncTarget::access(f.access), SyncTarget::access(other_access)],
            |_, _, _, _| panic!("accepted orders must not be sent again"), |_, _, _| Ok("Retrieved again".into())).unwrap();
    }

    #[test]
    fn account_and_group_sync_send_only_the_selected_accounts_orders_before_retrieval() {
        for group in [false, true] {
            let f = Fixture::new();
            let targets = f.ctx.edit_vault(|v| {
                let mut access = v.accesses[0].clone(); access.id = v.new_id();
                let mut account = v.accounts[0].clone(); account.id = v.new_id(); account.access_id = access.id;
                let other_id = account.id;
                v.accesses.push(access); v.accounts.push(account);
                let account_ids: Vec<_> = v.accounts.iter().map(|a| a.id).collect();
                for id in account_ids { v.queue_transfer(id, crate::business_tests::standing().transfer, false, false)?; }
                let group_id = v.create_group("Selected accounts")?;
                v.assign_group(f.account, Some(group_id))?;
                v.assign_group(other_id, Some(group_id))?;
                Ok(group_id)
            }).map(|group_id| if group { group_targets(&f.ctx, group_id).unwrap() } else {
                let (account, access) = account_and_access(&f.ctx, f.account).unwrap();
                vec![SyncTarget { access_id: access.id, accounts: Some(vec![(account, f.account)]) }]
            }).unwrap();
            let selected: Vec<_> = targets.iter().flat_map(|t| t.accounts.as_ref().unwrap().iter().map(|(_, id)| *id)).collect();
            let sent = std::cell::RefCell::new(Vec::new());
            sync_targets_with(&f.ctx, targets, |_, account, _, _| {
                assert!(selected.contains(&account)); sent.borrow_mut().push(account); Ok("Accepted".into())
            }, |_, _, accounts| {
                assert_eq!(sent.borrow().len(), selected.len(), "finish all selected payments before retrieval");
                assert!(accounts.unwrap().iter().all(|(_, id)| selected.contains(id)));
                Ok("Retrieved".into())
            }).unwrap();
            for order in f.reload().payment_orders {
                assert_eq!(order.status, if selected.contains(&order.account_id) { OrderStatus::Sent } else { OrderStatus::Queued });
            }
        }
    }

    #[test]
    fn failed_or_cancelled_payment_phase_stops_before_retrieval() {
        for cancel in [false, true] {
            let f = Fixture::new(); f.queue(); f.queue();
            let mut calls = 0;
            assert!(sync_targets_with(&f.ctx, vec![SyncTarget::access(f.access)], |ctx, _, _, _| {
                calls += 1;
                if cancel { ctx.bridge.cancelled.store(true, Ordering::Release); Ok("Accepted before cancellation".into()) }
                else { Err("Bank reply lost".into()) }
            }, |_, _, _| panic!("retrieval must wait for a successful payment phase")).is_err());
            assert_eq!(calls, 1);
            let saved = f.reload();
            assert_eq!(saved.payment_orders[0].status, if cancel { OrderStatus::Sent } else { OrderStatus::NeedsReview });
            assert_eq!(saved.payment_orders[1].status, OrderStatus::Queued);
        }
        let f = Fixture::new();
        f.ctx.bridge.cancelled.store(true, Ordering::Release);
        assert!(sync_targets_with(&f.ctx, vec![SyncTarget::access(f.access)],
            |_, _, _, _| panic!("cancelled"), |_, _, _| panic!("no retrieval even with an empty queue")).is_err());
    }

    #[test]
    fn activity_records_bank_acceptance_and_uncertain_outcomes_without_raw_messages() {
        for ok in [true,false] {
            let f=Fixture::new();let id=f.queue();
            let result=submit_order_with(&f.ctx,id,|_,_,_,_| if ok {Ok("synthetic-private-bank-reply".into())} else {Err("synthetic-private-bank-reply".into())});
            assert_eq!(result.is_ok(),ok);
            let v=f.reload();let event=v.history.entries.iter().find(|e|e.transfer.is_some()).unwrap();
            assert_eq!(event.status,if ok {crate::history::Status::Success} else {crate::history::Status::NeedsReview});
            assert_eq!(event.transfer.as_ref().unwrap().order_id,id);
            assert!(!serde_json::to_string(&v.history).unwrap().contains("synthetic-private-bank-reply"));
            f.ctx.edit_vault(|v|v.delete_payment_order(id)).unwrap();
            assert!(f.reload().history.entries.iter().any(|e|e.transfer.as_ref().is_some_and(|t|t.order_id==id)));
        }
    }
    #[test]
    fn queue_survives_restart_sends_once_and_only_for_matching_access() {
        let f = Fixture::new(); let id = f.queue();
        *f.ctx.vault.lock().unwrap() = f.reload();
        assert_eq!(dispatch_queued_with(&f.ctx, &[SyncTarget::access(9999)], |_, _, _, _| panic!("wrong account")).unwrap(), 0);
        let mut calls = 0;
        assert_eq!(dispatch_queued_with(&f.ctx, &[SyncTarget::access(f.access)], |_, account, t, scheduled| {
            calls += 1; assert_eq!(account, f.account); assert!(!scheduled);
            let saved = f.reload();
            assert_eq!(saved.payment_orders[0].status, OrderStatus::Submitting);
            assert_eq!(t.end_to_end_id, saved.payment_orders[0].transfer.end_to_end_id);
            assert!(!t.end_to_end_id.is_empty());
            Ok("Bank accepted".into())
        }).unwrap(), 1);
        assert_eq!(calls, 1); assert_eq!(f.reload().payment_orders[0].status, OrderStatus::Sent);
        *f.ctx.vault.lock().unwrap() = f.reload();
        assert_eq!(dispatch_queued_with(&f.ctx, &[SyncTarget::access(f.access)], |_, _, _, _| panic!("duplicate payment")).unwrap(), 0);
        assert!(submit_order_with(&f.ctx, id, |_, _, _, _| panic!("duplicate payment")).is_err());
    }
    #[test]
    fn lost_reply_stops_batch_and_never_retries_uncertain_order() {
        let f = Fixture::new(); let first = f.queue(); let second = f.queue();
        let mut calls = 0;
        assert!(dispatch_queued_with(&f.ctx, &[SyncTarget::access(f.access)], |_, _, _, _| { calls += 1; Err("Reply lost".into()) }).is_err());
        assert_eq!(calls, 1);
        let loaded = f.reload();
        assert_eq!(loaded.payment_orders[0].status, OrderStatus::NeedsReview);
        assert_eq!(loaded.payment_orders[1].status, OrderStatus::Queued);
        assert_eq!(loaded.queued_orders_for_access(f.access), vec![second]);
        *f.ctx.vault.lock().unwrap() = loaded;
        assert!(submit_order_with(&f.ctx, first, |_, _, _, _| panic!("uncertain order retried")).is_err());
    }
    #[test]
    fn interrupted_or_unpersisted_sends_are_not_retried() {
        let f = Fixture::new(); let id = f.queue();
        // Simulate a crash after persisting the dispatch marker.
        f.ctx.edit_vault(|v| { v.payment_orders[0].status = OrderStatus::Submitting; Ok(()) }).unwrap();
        *f.ctx.vault.lock().unwrap() = f.reload();
        assert_eq!(dispatch_queued_with(&f.ctx, &[SyncTarget::access(f.access)], |_, _, _, _| panic!("crashed order retried")).unwrap(), 0);
        f.ctx.edit_vault(|v| { v.payment_orders[0].status = OrderStatus::Queued; Ok(()) }).unwrap();
        *f.ctx.file.lock().unwrap() = Some(VaultFile::create(f.path.join("invalid-child"), "test").unwrap());
        assert!(submit_order_with(&f.ctx, id, |_, _, _, _| panic!("sent without durable marker")).is_err());
        assert_eq!(f.ctx.vault.lock().unwrap().payment_orders[0].status, OrderStatus::Queued);
    }
    #[test]
    fn successful_send_with_failed_final_save_leaves_safe_disk_marker() {
        let f = Fixture::new(); let id = f.queue();
        assert!(submit_order_with(&f.ctx, id, |ctx, _, _, _| {
            *ctx.file.lock().unwrap() = Some(VaultFile::create(f.path.join("invalid-child"), "test").unwrap());
            Ok("Accepted".into())
        }).is_err());
        assert_eq!(f.ctx.vault.lock().unwrap().payment_orders[0].status, OrderStatus::Sent);
        let loaded = f.reload(); assert_eq!(loaded.payment_orders[0].status, OrderStatus::Submitting);
        assert!(loaded.queued_orders_for_access(f.access).is_empty());
    }
    #[test]
    fn cancelled_sync_and_expired_scheduled_orders_do_not_send() {
        let f = Fixture::new(); f.queue();
        f.ctx.bridge.cancelled.store(true, Ordering::Release);
        assert!(dispatch_queued_with(&f.ctx, &[SyncTarget::access(f.access)], |_, _, _, _| panic!("cancelled")).is_err());
        assert_eq!(f.reload().payment_orders[0].status, OrderStatus::Queued);
        f.ctx.bridge.cancelled.store(false, Ordering::Release);
        f.ctx.edit_vault(|v| {
            v.payment_orders[0].scheduled = true;
            v.payment_orders[0].transfer.execution_date = Some(Local::now().date_naive()); Ok(())
        }).unwrap();
        assert!(dispatch_queued_with(&f.ctx, &[SyncTarget::access(f.access)], |_, _, _, _| panic!("expired scheduled order")).is_err());
        assert_eq!(f.reload().payment_orders[0].status, OrderStatus::NeedsReview);
    }
    #[test]
    fn immediate_dispatch_submits_only_the_selected_order() {
        let f = Fixture::new(); let deferred = f.queue(); let immediate = f.queue();
        submit_order_with(&f.ctx, immediate, |_, _, _, _| Ok("Sent immediately".into())).unwrap();
        assert_eq!(f.reload().queued_orders_for_access(f.access), vec![deferred]);
        assert_eq!(f.reload().payment_orders[1].status, OrderStatus::Sent);
    }
    #[test]
    fn failed_save_rolls_back_order_and_opted_in_recipient_together() {
        let f = Fixture::new();
        let before = serde_json::to_vec(&*f.ctx.vault.lock().unwrap()).unwrap();
        *f.ctx.file.lock().unwrap() = Some(VaultFile::create(f.path.join("invalid-child"), "test").unwrap());
        assert!(f.ctx.edit_vault(|v| v.queue_transfer(f.account, crate::business_tests::standing().transfer, false, true)).is_err());
        assert_eq!(serde_json::to_vec(&*f.ctx.vault.lock().unwrap()).unwrap(), before);
        assert_eq!(serde_json::to_vec(&f.reload()).unwrap(), before);
    }
}
