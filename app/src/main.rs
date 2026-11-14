// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod ui;
use ui::{AppWindow, state::*};

mod accounts_ui;
mod vaults;
mod payee_ui;
mod banks;
mod appearance;
mod localization;
mod product_id;
mod bankx_import;
mod bridge;
mod categorize;
mod demo;
mod export;
mod jobs;
mod payments;
mod paypal;
mod payments_ui;
mod transactions_ui;
mod groups;
mod private_file;
mod store;
mod view;
mod window_state;
#[cfg(test)]
mod business_tests;
#[cfg(test)]
mod showcase;

use bridge::{Answer, Bridge};
use chrono::Local;
use fints::{Interval, SepaTransfer, StandingOrder};
use jobs::Ctx;
use crate::ui::{Model, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use store::{AccessKind, BankAccess, Rule, Vault, VaultFile};
use zeroize::Zeroizing;

thread_local! {
    static VIEW: RefCell<view::ViewState> = RefCell::new(view::ViewState::default());
    static CTX: RefCell<Option<Ctx>> = const { RefCell::new(None) };
    static CONFIRM: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    static CONFIRM_CANCEL: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    static BANK_HITS: RefCell<Vec<&'static banks::Bank>> = const { RefCell::new(Vec::new()) };
    static AUTO_NAME: RefCell<String> = const { RefCell::new(String::new()) };
}

fn ctx() -> Ctx {
    CTX.with(|c| c.borrow().clone().expect("context initialised"))
}

pub fn refresh_view(app: &AppWindow) {
    let ctx = ctx();
    let v = ctx.vault.lock().unwrap();
    VIEW.with(|vs| view::refresh_all(app, &v, &mut vs.borrow_mut()));
}

pub fn toast(app: &AppWindow, text: &str, error: bool) {
    let st = app.global::<AppState>();
    st.set_toast(Toast { visible: true, text: text.into(), error });
    let weak = app.as_weak();
    let shown = text.to_string();
    let secs = if error { 12 } else { 6 } + text.len() as u64 / 60;
    crate::ui::Timer::single_shot(std::time::Duration::from_secs(secs), move || {
        if let Some(app) = weak.upgrade() {
            let st = app.global::<AppState>();
            if st.get_toast().text == shown.as_str() {
                st.set_toast(Toast::default());
            }
        }
    });
}

fn confirm(app: &AppWindow, title: &str, text: &str, danger: bool, action: impl FnOnce() + 'static) {
    let st = app.global::<AppState>();
    st.set_confirm_danger(danger);
    st.set_confirm_title(title.into());
    st.set_confirm_text(text.into());
    st.set_confirm_ok_text(SharedString::default());
    st.set_show_confirm(true);
    CONFIRM.with(|c| *c.borrow_mut() = Some(Box::new(action)));
    CONFIRM_CANCEL.with(|c| *c.borrow_mut() = None);
}

/// Yes/no question from the worker thread (answered through the bridge).
fn confirm_blocking(app: &AppWindow, title: &str, text: &str, bridge: bridge::Bridge) {
    let st = app.global::<AppState>();
    st.set_confirm_danger(false);
    st.set_confirm_title(title.into());
    st.set_confirm_text(text.into());
    st.set_confirm_ok_text(i18n::tr("Bestätigen").into());
    st.set_show_confirm(true);
    let b = bridge.clone();
    CONFIRM.with(|c| *c.borrow_mut() = Some(Box::new(move || b.answer(bridge::Answer::Confirmed))));
    CONFIRM_CANCEL.with(|c| *c.borrow_mut() = Some(Box::new(move || bridge.answer(bridge::Answer::Cancel))));
}

fn refresh_readers(app: &AppWindow) {
    let readers = cardreader::list_readers().unwrap_or_default();
    let st = app.global::<AppState>();
    let names: Vec<SharedString> = readers.iter().map(|r| SharedString::from(r.name.as_str())).collect();
    st.set_cyberjack_found(readers.iter().any(|r| r.cyberjack));
    let preferred = ctx().vault.lock().unwrap().chiptan_reader.clone();
    let tan_idx = readers
        .iter()
        .position(|r| !preferred.is_empty() && r.name == preferred)
        .or_else(|| readers.iter().position(|r| r.cyberjack))
        .unwrap_or(0);
    st.set_tan_reader(tan_idx as i32);
    st.set_af_reader(readers.iter().position(|r| r.cyberjack).unwrap_or(0) as i32);
    if names.is_empty() {
        st.set_reader_names(ModelRc::new(VecModel::from(vec![SharedString::from(i18n::tr("(kein Kartenleser gefunden)"))])));
    } else {
        st.set_reader_names(ModelRc::new(VecModel::from(names)));
    }
}

fn selected_reader(app: &AppWindow, idx: i32) -> Option<String> {
    let st = app.global::<AppState>();
    let r = st.get_reader_names().row_data(idx.max(0) as usize)?.to_string();
    if r.starts_with('(') { None } else { Some(r) }
}

fn after_unlock(app: &AppWindow, mut vault: Vault, file: VaultFile) {
    let import_result = if file.path() == store::default_path() { bankx_import::pending(&mut vault, &file) } else { Ok(None) };
    let st = app.global::<AppState>();
    st.set_data_path(file.path().display().to_string());
    st.set_product_id(vault.product_id.clone());
    st.set_product_version(if vault.product_version.is_empty() { env!("CARGO_PKG_VERSION").into() } else { vault.product_version.clone() });
    let c = ctx();
    *c.vault.lock().unwrap() = vault;
    *c.file.lock().unwrap() = Some(file);
    st.set_unlocked(true);
    st.set_unlock_error(SharedString::default());
    refresh_view(app);
    #[cfg(not(test))]
    refresh_readers(app);
    match import_result {
        Ok(Some(message)) => toast(app, &message, false),
        Err(e) => toast(app, &i18n::tr_format!("Bank-X-Übernahme fehlgeschlagen: {e}"), true),
        Ok(None) => {},
    }
}

fn edit_vault(app: &AppWindow, edit: impl FnOnce(&mut Vault) -> Result<(), String>) -> bool {
    let c = ctx();
    if c.running.load(std::sync::atomic::Ordering::Acquire) { return false; }
    let result = c.edit_vault(edit);
    match result {
        Ok(()) => { refresh_view(app); true },
        Err(e) => { toast(app, &e, true); false },
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let app = AppWindow::new().expect("UI");
    appearance::install(&app);
    localization::install(&app);
    accounts_ui::install(&app);
    transactions_ui::install(&app);
    let st = app.global::<AppState>();
    st.set_tx_period(1);
    st.set_so_first(Local::now().date_naive().format("%d.%m.%Y").to_string());

    let bridge = Bridge::new(app.as_weak());
    CTX.with(|c| {
        *c.borrow_mut() = Some(Ctx { vault: Arc::new(Mutex::new(Vault::default())), file: Arc::new(Mutex::new(None)), bridge: bridge.clone(), running: Arc::new(std::sync::atomic::AtomicBool::new(false)) })
    });

    vaults::install(&app);
    payee_ui::install(&app, &bridge);

    // ---------------------------------------------------------------- navigation
    {
        let weak = app.as_weak();
        st.on_navigate(move |p| {
            let app = weak.unwrap();
            app.global::<AppState>().set_page(p);
            if p == 7 {
                refresh_readers(&app);
            }
        });
    }
    st.on_refresh_all(|| ctx().run(i18n::tr("Kontenrundruf …"), jobs::sync_all));
    st.on_refresh_group(|id| ctx().run(i18n::tr("Gruppe wird aktualisiert …"), move |c| jobs::sync_group(c, id as u32)));
    {
        let weak = app.as_weak();
        st.on_overview_group_changed(move |tile, index| {
            let app = weak.unwrap();
            let c = ctx();
            let v = c.vault.lock().unwrap();
            VIEW.with(|vs| view::change_overview_group(&app, &v, &mut vs.borrow_mut(), tile, index));
        });
    }
    {
        let weak = app.as_weak();
        st.on_move_account(move |source, target, after| {
            edit_vault(&weak.unwrap(), |v| v.move_account(source as u32, target as u32, after))
        });
    }
    {
        let weak = app.as_weak();
        st.on_create_group(move |name| {
            let app = weak.unwrap();
            if edit_vault(&app, |v| v.create_group(&name).map(|_| ())) {
                app.global::<AppState>().set_new_group_name(SharedString::default());
            }
        });
    }
    {
        let weak = app.as_weak();
        st.on_rename_group(move |id, name| { edit_vault(&weak.unwrap(), |v| v.rename_group(id as u32, &name)); });
    }
    {
        let weak = app.as_weak();
        st.on_assign_group(move |account, index| {
            edit_vault(&weak.unwrap(), |v| {
                let id = if index == 0 { None } else { Some(v.groups.get((index - 1) as usize).ok_or(i18n::tr("Gruppe nicht gefunden"))?.id) };
                v.assign_group(account as u32, id)
            });
        });
    }
    {
        let weak = app.as_weak();
        st.on_delete_group(move |id| {
            let app = weak.unwrap();
            let weak = app.as_weak();
            confirm(&app, i18n::tr("Gruppe entfernen"), i18n::tr("Die Konten bleiben erhalten und werden unter „Ohne Gruppe“ angezeigt."), true, move || {
                edit_vault(&weak.unwrap(), |v| v.delete_group(id as u32));
            });
        });
    }
    {
        let weak = app.as_weak();
        st.on_open_account(move |id| {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            let pos = VIEW.with(|vs| vs.borrow().account_filter.iter().position(|&a| a as i32 == id));
            st.set_tx_account(pos.map(|p| p as i32 + 1).unwrap_or(0));
            st.set_page(2);
            let c = ctx();
            let v = c.vault.lock().unwrap();
            VIEW.with(|vs| view::refresh_transactions(&app, &v, &vs.borrow()));
        });
    }

    // ---------------------------------------------------------------- transactions
    {
        let weak = app.as_weak();
        st.on_tx_filter_changed(move || {
            let app = weak.unwrap();
            let c = ctx();
            let v = c.vault.lock().unwrap();
            VIEW.with(|vs| view::refresh_transactions(&app, &v, &vs.borrow()));
        });
    }
    {
        let weak = app.as_weak();
        st.on_tx_select(move |i| {
            let app = weak.unwrap();
            let c = ctx();
            let v = c.vault.lock().unwrap();
            view::show_detail(&app, &v, i.max(0) as usize);
        });
    }
    {
        let weak = app.as_weak();
        st.on_set_category(move |i, cat| {
            let app = weak.unwrap();
            let c = ctx();
            {
                let mut v = c.vault.lock().unwrap();
                let Some(t) = v.transactions.get_mut(i as usize) else { return };
                t.category = Some(cat.to_string());
                let name = t.tx.counterparty_name.to_lowercase();
                if !name.is_empty() {
                    v.rules.retain(|r| r.pattern != name);
                    v.rules.push(Rule { pattern: name, category: cat.to_string() });
                }
            }
            let _ = c.save();
            refresh_view(&app);
            let v = c.vault.lock().unwrap();
            view::show_detail(&app, &v, i as usize);
        });
    }
    {
        let weak = app.as_weak();
        st.on_export_csv(move || {
            let app = weak.unwrap();
            let idx = visible_tx(&app);
            let c = ctx();
            let v = c.vault.lock().unwrap();
            match export::csv(&v, &idx) {
                Ok(p) => toast(&app, &i18n::tr_format!("{} Umsätze exportiert:\n{}", idx.len(), p.display()), false),
                Err(e) => toast(&app, &e, true),
            }
        });
    }
    {
        let weak = app.as_weak();
        st.on_export_camt(move || {
            let app = weak.unwrap();
            let idx = visible_tx(&app);
            let c = ctx();
            let v = c.vault.lock().unwrap();
            match export::camt052(&v, &idx) {
                Ok(p) => toast(&app, &i18n::tr_format!("{} Umsätze als camt.052 exportiert:\n{}", idx.len(), p.display()), false),
                Err(e) => toast(&app, &e, true),
            }
        });
    }

    payments_ui::install(&app);

    // ---------------------------------------------------------------- payments
    {
        let weak = app.as_weak();
        st.on_check_iban(move |iban| {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            let clean: String = iban.chars().filter(|c| !c.is_whitespace()).collect();
            if clean.len() < 5 {
                st.set_tf_iban_hint(SharedString::default());
                st.set_tf_iban_error(false);
            } else if fints::pain::iban_valid(&clean) {
                st.set_tf_iban_hint(i18n::tr("IBAN gültig").into());
                st.set_tf_iban_error(false);
            } else {
                st.set_tf_iban_hint(i18n::tr("Prüfziffer passt nicht – bitte IBAN kontrollieren").into());
                st.set_tf_iban_error(true);
            }
        });
    }
    {
        let weak = app.as_weak();
        st.on_send_transfer(move || {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            let Some(account_id) = VIEW.with(|vs| vs.borrow().sepa_accounts.get(st.get_tf_account().max(0) as usize).copied()) else {
                toast(&app, i18n::tr("Bitte ein Auftraggeberkonto wählen."), true);
                return;
            };
            let (t, scheduled) = match transfer_from_form(&app) {
                Ok(x) => x,
                Err(e) => {
                    toast(&app, &e, true);
                    return;
                }
            };
            let text = i18n::tr_format!(
                "{} an {}\n{}\n„{}“{}",
                view::fmt_eur(t.amount),
                t.creditor_name,
                t.creditor_iban,
                t.purpose,
                t.execution_date.map(|d| i18n::tr_format!("\nAusführung am {}", view::fmt_date(d))).unwrap_or_default()
            );
            let save_recipient = st.get_tf_save_recipient();
            let weak2 = app.as_weak();
            confirm(&app, i18n::tr("Überweisung senden?"), &text, false, move || {
                ctx().run(i18n::tr("Überweisung wird übermittelt …"), move |c| {
                    let r = jobs::transfer(c, account_id, t, scheduled, save_recipient);
                    if r.is_ok() {
                        let _ = weak2.upgrade_in_event_loop(|app| clear_transfer_form(&app));
                    }
                    r
                });
            });
        });
    }
    {
        let weak = app.as_weak();
        st.on_send_standing(move || {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            let Some(account_id) = VIEW.with(|vs| vs.borrow().sepa_accounts.get(st.get_tf_account().max(0) as usize).copied()) else {
                toast(&app, i18n::tr("Bitte ein Auftraggeberkonto wählen."), true);
                return;
            };
            let (mut t, _) = match transfer_from_form(&app) {
                Ok(x) => x,
                Err(e) => {
                    toast(&app, &e, true);
                    return;
                }
            };
            t.execution_date = None;
            let Some(first) = view::parse_date(&st.get_so_first()) else {
                toast(&app, i18n::tr("Bitte das Datum der ersten Ausführung als TT.MM.JJJJ angeben."), true);
                return;
            };
            let last = if st.get_so_last().trim().is_empty() {
                None
            } else {
                let Some(date) = view::parse_date(&st.get_so_last()) else {
                    toast(&app, i18n::tr("Bitte ein gültiges Enddatum als TT.MM.JJJJ angeben."), true);
                    return;
                };
                Some(date)
            };
            let Ok(day) = st.get_so_day().trim().parse::<u32>() else {
                toast(&app, i18n::tr("Bitte einen gültigen Ausführungstag angeben."), true);
                return;
            };
            let (interval, every) = match st.get_so_interval() {
                1 => (Interval::Monthly, 3),
                2 => (Interval::Monthly, 6),
                3 => (Interval::Monthly, 12),
                4 => (Interval::Weekly, 1),
                _ => (Interval::Monthly, 1),
            };
            let so = StandingOrder {
                order_id: String::new(),
                execution_time: None,
                transfer: t,
                first_date: first,
                last_date: last,
                interval,
                every,
                day,
                raw_pain: Vec::new(),
                descriptor: String::new(),
            };
            if let Err(e) = fints::pain::validate_standing_schedule(&so) {
                toast(&app, &e.to_string(), true);
                return;
            }
            let text = i18n::tr_format!("{} an {}, {}", view::fmt_eur(so.transfer.amount), so.transfer.creditor_name, view::schedule_text(&so));
            confirm(&app, i18n::tr("Dauerauftrag anlegen?"), &text, false, move || {
                ctx().run(i18n::tr("Dauerauftrag wird angelegt …"), move |c| jobs::create_standing(c, account_id, so));
            });
        });
    }
    st.on_fetch_standing(|| ctx().run(i18n::tr("Daueraufträge werden abgerufen …"), jobs::fetch_standing));
    {
        let weak = app.as_weak();
        st.on_delete_standing(move |i| {
            let app = weak.unwrap();
            let c = ctx();
            let name = c.vault.lock().unwrap().standing.get(i as usize).map(|s| s.order.transfer.creditor_name.clone()).unwrap_or_default();
            confirm(&app, i18n::tr("Dauerauftrag löschen?"), &i18n::tr_format!("Der Dauerauftrag an {name} wird bei der Bank gelöscht."), true, move || {
                ctx().run(i18n::tr("Dauerauftrag wird gelöscht …"), move |c| jobs::delete_standing(c, i as usize));
            });
        });
    }

    // ---------------------------------------------------------------- analysis
    {
        let weak = app.as_weak();
        st.on_analysis_shift(move |d| {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            st.set_analysis_offset((st.get_analysis_offset() + d).min(0));
            let c = ctx();
            let v = c.vault.lock().unwrap();
            view::refresh_analysis(&app, &v);
        });
    }

    // ---------------------------------------------------------------- bank accesses
    {
        let weak = app.as_weak();
        st.on_new_access(move || {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            st.set_af_name(SharedString::default());
            st.set_af_blz(SharedString::default());
            st.set_af_user(SharedString::default());
            st.set_af_customer(SharedString::default());
            st.set_af_paypal_password(Default::default());
            st.set_af_paypal_signature(Default::default());
            st.set_af_paypal_sandbox(false);
            st.set_af_statement_format(0);
            st.set_af_url(SharedString::default());
            st.set_af_record("1".into());
            st.set_bank_query(SharedString::default());
            st.set_bank_picked(SharedString::default());
            st.set_bank_results(ModelRc::new(VecModel::from(Vec::<BankRow>::new())));
            st.set_af_open(true);
            refresh_readers(&app);
        });
    }
    {
        let weak = app.as_weak();
        st.on_bank_search(move |q| {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            let chip = st.get_af_kind() == 1 || st.get_af_kind() == 2;
            st.set_bank_picked(SharedString::default());
            let hits = banks::search(&q, chip, 8);
            let rows: Vec<BankRow> = hits
                .iter()
                .enumerate()
                .map(|(i, b)| BankRow {
                    index: i as i32,
                    name: b.name.into(),
                    detail: i18n::tr_format!("BLZ {} · {} · {}", b.blz, b.city, b.bic),
                    endpoint: if chip { b.host } else { b.url.trim_start_matches("https://") }.into(),
                })
                .collect();
            BANK_HITS.with(|h| *h.borrow_mut() = hits);
            st.set_bank_results(ModelRc::new(VecModel::from(rows)));
        });
    }
    {
        let weak = app.as_weak();
        st.on_bank_pick(move |i| {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            let Some(b) = BANK_HITS.with(|h| h.borrow().get(i as usize).copied()) else { return };
            let chip = st.get_af_kind() == 1 || st.get_af_kind() == 2;
            st.set_af_blz(b.blz.into());
            st.set_af_url(if chip { b.host } else { b.url }.into());
            let current = st.get_af_name().trim().to_string();
            if current.is_empty() || AUTO_NAME.with(|n| *n.borrow() == current) {
                st.set_af_name(b.name.into());
                AUTO_NAME.with(|n| *n.borrow_mut() = b.name.to_string());
            }
            st.set_bank_query(b.name.into());
            let hint = if chip { None } else { banks::login_hint(b) };
            st.set_bank_picked(i18n::tr_format!("✓ {} · BLZ {} übernommen{}", b.name, b.blz, hint.map(|h| format!(" – {h}")).unwrap_or_default()));
            st.set_bank_results(ModelRc::new(VecModel::from(Vec::<BankRow>::new())));
        });
    }
    {
        let weak = app.as_weak();
        st.on_read_card(move || {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            let Some(reader) = selected_reader(&app, st.get_af_reader()) else {
                toast(&app, i18n::tr("Kein Kartenleser ausgewählt."), true);
                return;
            };
            let record: u8 = st.get_af_record().trim().parse().unwrap_or(1);
            let weak = app.as_weak();
            st.set_busy(true);
            st.set_busy_text(i18n::tr("Lese Bankdaten von der Chipkarte …").into());
            if st.get_af_kind() == 2 {
                std::thread::spawn(move || {
                    let r = jobs::read_rdh_card(&ctx(), &reader, record);
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        let st = app.global::<AppState>();
                        st.set_busy(false);
                        match r {
                            Ok((c, number)) => {
                                st.set_af_blz(c.blz.clone());
                                st.set_af_user(c.user_id.clone());
                                st.set_af_customer(if c.customer_id == c.user_id { String::new() } else { c.customer_id.clone() });
                                st.set_af_record(c.record.to_string());
                                let host = if c.host.trim().is_empty() {
                                    banks::by_blz(&c.blz).map(|x| x.host.to_string()).unwrap_or_default()
                                } else {
                                    c.host.clone()
                                };
                                st.set_af_url(host);
                                if st.get_af_name().is_empty() {
                                    let bank = if c.bank_name.is_empty() { i18n::tr_format!("BLZ {}", c.blz) } else { c.bank_name.clone() };
                                    st.set_af_name(i18n::tr_format!("{bank} (RDH-Chipkarte)"));
                                }
                                toast(
                                    &app,
                                    &i18n::tr_format!("Karte {number} gelesen: RDH-{} · BLZ {} · Kennung {}", c.profile, c.blz, c.user_id),
                                    false,
                                );
                            }
                            Err(e) => toast(&app, &e, true),
                        }
                    });
                });
                return;
            }
            std::thread::spawn(move || {
                let r = jobs::read_card(&reader, record);
                let _ = weak.upgrade_in_event_loop(move |app| {
                    let st = app.global::<AppState>();
                    st.set_busy(false);
                    match r {
                        Ok(b) => {
                            st.set_af_blz(b.blz.clone());
                            st.set_af_user(b.user_id.clone());
                            let host = if b.host.trim().is_empty() { banks::by_blz(&b.blz).map(|x| x.host.to_string()).unwrap_or_default() } else { b.host.clone() };
                            st.set_af_url(host);
                            if st.get_af_name().is_empty() {
                                st.set_af_name(i18n::tr_format!("{} (Chipkarte)", b.short_name));
                            }
                            toast(&app, &i18n::tr_format!("Karte gelesen: {} · BLZ {}", b.short_name, b.blz), false);
                        }
                        Err(e) => toast(&app, &e, true),
                    }
                });
            });
        });
    }
    {
        let weak = app.as_weak();
        st.on_save_access(move || {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            if st.get_af_kind() == 3 {
                let c = ctx();
                match paypal::save_access(&app, &c) {
                    Ok(id) => {
                        st.set_af_user(Default::default());
                        st.set_af_paypal_password(Default::default());
                        st.set_af_paypal_signature(Default::default());
                        st.set_af_open(false);
                        refresh_view(&app);
                        c.run(i18n::tr("PayPal-Guthaben und Umsätze werden abgerufen …"), move |c| jobs::sync_access(c, id));
                    }
                    Err(e) => toast(&app, &e, true),
                }
                return;
            }
            let blz: String = st.get_af_blz().chars().filter(|c| c.is_ascii_digit()).collect();
            let user = st.get_af_user().trim().to_string();
            let url = st.get_af_url().trim().to_string();
            let chip = st.get_af_kind() == 1 || st.get_af_kind() == 2;
            if blz.len() != 8 {
                toast(&app, i18n::tr("Die Bankleitzahl muss 8 Ziffern haben."), true);
                return;
            }
            if user.is_empty() {
                toast(&app, i18n::tr("Bitte die Benutzerkennung angeben."), true);
                return;
            }
            if !chip && fints::transport::validate_https_url(&url).is_err() {
                toast(&app, i18n::tr("Bitte die FinTS-URL Ihrer Bank (https://…) angeben."), true);
                return;
            }
            let kind = if chip {
                let Some(reader) = selected_reader(&app, st.get_af_reader()) else {
                    toast(&app, i18n::tr("Kein Kartenleser ausgewählt."), true);
                    return;
                };
                let record = st.get_af_record().trim().parse().unwrap_or(1);
                if st.get_af_kind() == 2 {
                    AccessKind::RdhCard { reader, record }
                } else {
                    AccessKind::ChipCard { reader, record }
                }
            } else {
                AccessKind::PinTan
            };
            let name = if st.get_af_name().trim().is_empty() { format!("Bank {blz}") } else { st.get_af_name().trim().to_string() };
            let c = ctx();
            let id = {
                let mut v = c.vault.lock().unwrap();
                let id = v.new_id();
                v.accesses.push(BankAccess {
                    id,
                    name,
                    kind,
                    config: fints::Config {
                        blz,
                        user_id: user,
                        customer_id: st.get_af_customer().trim().to_string(),
                        statement_format: match st.get_af_statement_format() {
                            1 => fints::bank_profile::StatementFormat::Camt,
                            2 => fints::bank_profile::StatementFormat::Mt940,
                            _ => fints::bank_profile::StatementFormat::Auto,
                        },
                        url,
                        ..Default::default()
                    },
                    state: fints::State::default(),
                    saved_pin: None,
                });
                id
            };
            st.set_af_open(false);
            let _ = c.save();
            refresh_view(&app);
            c.run(i18n::tr("Bankzugang wird eingerichtet …"), move |c| jobs::sync_access(c, id));
        });
    }
    {
        let weak = app.as_weak();
        st.on_delete_access(move |id| {
            let app = weak.unwrap();
            let c = ctx();
            let name = c.vault.lock().unwrap().access(id as u32).map(|a| a.name.clone()).unwrap_or_default();
            let weak = app.as_weak();
            confirm(&app, i18n::tr("Bankzugang entfernen?"), &i18n::tr_format!("„{name}“ mit allen lokal gespeicherten Konten und Umsätzen entfernen? Bei der Bank wird nichts geändert."), true, move || {
                let c = ctx();
                c.vault.lock().unwrap().remove_access(id as u32);
                let _ = c.save();
                if let Some(app) = weak.upgrade() {
                    refresh_view(&app);
                }
            });
        });
    }
    {
        let weak = app.as_weak();
        st.on_forget_pin(move |id| {
            let app = weak.unwrap();
            let c = ctx();
            if let Some(a) = c.vault.lock().unwrap().accesses.iter_mut().find(|a| a.id == id as u32) {
                a.saved_pin = None;
            }
            let _ = c.save();
            refresh_view(&app);
            toast(&app, i18n::tr("Die gespeicherte PIN wurde gelöscht."), false);
        });
    }
    st.on_sync_access(|id| ctx().run(i18n::tr("Bankzugang wird synchronisiert …"), move |c| jobs::sync_access(c, id as u32)));
    st.on_reset_tan(|id| ctx().run(i18n::tr("TAN-Verfahren werden abgefragt …"), move |c| jobs::reset_tan(c, id as u32)));

    // ---------------------------------------------------------------- card reader
    {
        let weak = app.as_weak();
        st.on_reader_refresh(move || refresh_readers(&weak.unwrap()));
    }
    {
        let weak = app.as_weak();
        st.on_reader_diagnose(move || {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            let reader = selected_reader(&app, st.get_tan_reader());
            st.set_busy(true);
            st.set_busy_text(i18n::tr("Prüfe Kartenleser …").into());
            let weak = app.as_weak();
            std::thread::spawn(move || {
                let report = cardreader::diagnose(reader.as_deref());
                let _ = weak.upgrade_in_event_loop(move |app| {
                    let st = app.global::<AppState>();
                    st.set_busy(false);
                    st.set_reader_report(ModelRc::new(VecModel::from(report.into_iter().collect::<Vec<_>>())));
                    refresh_readers(&app);
                });
            });
        });
    }

    // ---------------------------------------------------------------- settings
    {
        let weak = app.as_weak();
        st.on_save_settings(move || {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            let c = ctx();
            {
                let mut v = c.vault.lock().unwrap();
                v.product_id = st.get_product_id().trim().to_string();
                v.product_version = st.get_product_version().trim().to_string();
                if let Some(r) = selected_reader(&app, st.get_tan_reader()) {
                    v.chiptan_reader = r;
                }
            }
            match c.save() {
                Ok(()) => toast(&app, i18n::tr("Einstellungen gespeichert."), false),
                Err(e) => toast(&app, &e, true),
            }
        });
    }
    {
        let weak = app.as_weak();
        st.on_change_password(move || {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            let (old, new) = (Zeroizing::new(st.get_old_password().to_string()), Zeroizing::new(st.get_new_password().to_string()));
            st.set_old_password(SharedString::default());
            st.set_new_password(SharedString::default());
            if new.chars().count() < 8 {
                toast(&app, i18n::tr("Das neue Passwort braucht mindestens 8 Zeichen."), true);
                return;
            }
            let c = ctx();
            let r = {
                let mut v = c.vault.lock().unwrap();
                c.file.lock().unwrap().as_mut().map(|f| f.change_password(&old, &new, &mut v)).unwrap_or(Err(i18n::tr("Tresor nicht geöffnet").into()))
            };
            match r {
                Ok(()) => {
                    st.set_old_password(SharedString::default());
                    st.set_new_password(SharedString::default());
                    toast(&app, i18n::tr("Tresor-Passwort geändert."), false);
                }
                Err(e) => toast(&app, &e, true),
            }
        });
    }
    {
        let weak = app.as_weak();
        st.on_load_demo(move || {
            let app = weak.unwrap();
            let c = ctx();
            demo::add_demo(&mut c.vault.lock().unwrap());
            let _ = c.save();
            refresh_view(&app);
            toast(&app, i18n::tr("Demo-Bankzugang mit Beispielumsätzen angelegt."), false);
        });
    }

    // ---------------------------------------------------------------- dialogs
    {
        let weak = app.as_weak();
        let b = bridge.clone();
        st.on_pin_ok(move || {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            let pin = st.get_pin_value().to_string();
            *b.pin_save_requested.lock().unwrap() = st.get_pin_offer_save() && st.get_pin_save();
            st.set_pin_value(SharedString::default());
            st.set_paste_note(SharedString::default());
            st.set_pin_save(false);
            st.set_show_pin(false);
            b.answer(Answer::Text(pin));
        });
    }
    {
        let weak = app.as_weak();
        st.on_pin_paste(move || {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            match clipboard_text() {
                Ok(text) => {
                    st.set_pin_value(text.as_str().into());
                    if !st.get_pin_masked() {
                        st.set_paste_note(i18n::tr("Eingefügt.").into());
                        return;
                    }
                    // do not leave the PIN lying around in the clipboard
                    let cleared = clear_clipboard_if(&text);
                    st.set_paste_note(
                        if cleared { i18n::tr("PIN eingefügt – die Zwischenablage wurde aus Sicherheitsgründen geleert.") } else { i18n::tr("PIN eingefügt.") }.into(),
                    );
                }
                Err(e) => st.set_paste_note(e),
            }
        });
    }
    {
        let weak = app.as_weak();
        st.on_tan_paste(move || {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            match clipboard_text() {
                Ok(text) => st.set_tan_value(text),
                Err(e) => toast(&app, &e, true),
            }
        });
    }
    {
        let weak = app.as_weak();
        let b = bridge.clone();
        st.on_pin_cancel(move || {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            st.set_pin_value(SharedString::default());
            st.set_paste_note(SharedString::default());
            st.set_show_pin(false);
            b.answer(Answer::Cancel);
        });
    }
    {
        let weak = app.as_weak();
        let b = bridge.clone();
        st.on_tan_ok(move || {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            let tan = st.get_tan_value().trim().to_string();
            if tan.is_empty() {
                return;
            }
            close_tan(&st);
            b.answer(Answer::Text(tan));
        });
    }
    {
        let weak = app.as_weak();
        let b = bridge.clone();
        st.on_tan_cancel(move || {
            close_tan(&weak.unwrap().global::<AppState>());
            b.answer(Answer::Cancel);
        });
    }
    {
        let weak = app.as_weak();
        let b = bridge.clone();
        st.on_tan_confirmed(move || {
            close_tan(&weak.unwrap().global::<AppState>());
            b.answer(Answer::Confirmed);
        });
    }
    {
        let weak = app.as_weak();
        let b = bridge.clone();
        st.on_tan_usb(move || {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            let Some(payload) = b.usb_payload.lock().unwrap().clone() else { return };
            let reader = selected_reader(&app, st.get_tan_reader());
            b.pinpad(true, i18n::tr("Bitte prüfen Sie Empfänger und Betrag im Display des Kartenlesers und bestätigen Sie dort mit OK."));
            let b2 = b.clone();
            let weak = app.as_weak();
            std::thread::spawn(move || {
                let r = cardreader::chiptan_usb(reader.as_deref(), &payload);
                b2.pinpad(false, "");
                let _ = weak.upgrade_in_event_loop(move |app| {
                    let st = app.global::<AppState>();
                    match r {
                        Ok(tan) => {
                            close_tan(&st);
                            b2.answer(Answer::Text(tan));
                        }
                        Err(e) => toast(&app, &format!("chipTAN USB: {e}"), true),
                    }
                });
            });
        });
    }
    {
        let weak = app.as_weak();
        st.on_flicker_speed(move |d| {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            st.set_flicker_hz((st.get_flicker_hz() + d).clamp(4, 40));
            bridge::restart_flicker(&app);
        });
    }
    {
        let weak = app.as_weak();
        let b = bridge.clone();
        st.on_choice_picked(move |k| {
            weak.unwrap().global::<AppState>().set_show_choice(false);
            b.answer(Answer::Text(k.to_string()));
        });
    }
    {
        let weak = app.as_weak();
        let b = bridge.clone();
        st.on_choice_cancel(move || {
            weak.unwrap().global::<AppState>().set_show_choice(false);
            b.answer(Answer::Cancel);
        });
    }
    {
        let weak = app.as_weak();
        st.on_confirm_ok(move || {
            weak.unwrap().global::<AppState>().set_show_confirm(false);
            CONFIRM_CANCEL.with(|c| *c.borrow_mut() = None);
            if let Some(f) = CONFIRM.with(|c| c.borrow_mut().take()) {
                f();
            }
        });
    }
    {
        let weak = app.as_weak();
        st.on_confirm_cancel(move || {
            weak.unwrap().global::<AppState>().set_show_confirm(false);
            CONFIRM.with(|c| *c.borrow_mut() = None);
            if let Some(f) = CONFIRM_CANCEL.with(|c| c.borrow_mut().take()) {
                f();
            }
        });
    }
    {
        let weak = app.as_weak();
        st.on_toast_close(move || weak.unwrap().global::<AppState>().set_toast(Toast::default()));
    }

    ui::run(app).expect("event loop");
}

/// Text from the system clipboard, trimmed (PINs/TANs are often copied with a trailing newline).
fn clipboard_text() -> Result<String, String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| i18n::tr_format!("Zwischenablage nicht verfügbar: {e}"))?;
    let text = cb.get_text().map_err(|_| i18n::tr("Die Zwischenablage enthält keinen Text.").to_string())?;
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(i18n::tr("Die Zwischenablage ist leer.").into());
    }
    if text.chars().count() > 64 || text.contains('\n') {
        return Err(i18n::tr("Der Inhalt der Zwischenablage sieht nicht nach einer PIN/TAN aus.").into());
    }
    Ok(text)
}

/// Clear the clipboard if it still holds `expected` (avoids wiping something copied in between).
fn clear_clipboard_if(expected: &str) -> bool {
    let Ok(mut cb) = arboard::Clipboard::new() else { return false };
    match cb.get_text() {
        Ok(t) if t.trim() == expected => cb.clear().is_ok(),
        _ => false,
    }
}

fn close_tan(st: &AppState) {
    bridge::stop_flicker();
    st.set_show_tan(false);
    st.set_tan_value(SharedString::default());
    st.set_tan_usb_possible(false);
}

fn visible_tx(app: &AppWindow) -> Vec<usize> {
    let rows = app.global::<AppState>().get_tx_rows();
    rows.iter().filter(|r| !r.is_header).map(|r| r.index as usize).collect()
}

fn transfer_from_form(app: &AppWindow) -> Result<(SepaTransfer, bool), String> {
    let st = app.global::<AppState>();
    let name = st.get_tf_name().trim().to_string();
    let iban: String = st.get_tf_iban().chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase();
    let amount = view::parse_amount(&st.get_tf_amount()).ok_or(i18n::tr("Bitte einen gültigen Betrag angeben."))?;
    let purpose = st.get_tf_purpose().trim().to_string();
    if name.is_empty() {
        return Err(i18n::tr("Bitte den Empfänger angeben.").into());
    }
    if !fints::pain::iban_valid(&iban) {
        return Err(i18n::tr("Die IBAN ist ungültig.").into());
    }
    if amount <= rust_decimal::Decimal::ZERO {
        return Err(i18n::tr("Der Betrag muss größer als 0 sein.").into());
    }
    if purpose.chars().count() > 140 {
        return Err(i18n::tr("Der Verwendungszweck ist länger als 140 Zeichen.").into());
    }
    let mode = st.get_tf_mode();
    let execution_date = if mode == 2 {
        let d = view::parse_date(&st.get_tf_date()).ok_or(i18n::tr("Bitte das Ausführungsdatum als TT.MM.JJJJ angeben."))?;
        if d <= Local::now().date_naive() {
            return Err(i18n::tr("Eine Terminüberweisung muss in der Zukunft liegen.").into());
        }
        Some(d)
    } else {
        None
    };
    Ok((
        SepaTransfer {
            debtor_name: String::new(),
            debtor_iban: String::new(),
            debtor_bic: String::new(),
            creditor_name: name,
            creditor_iban: iban,
            creditor_bic: st.get_tf_bic().trim().to_uppercase(),
            amount,
            purpose,
            end_to_end_id: String::new(),
            execution_date,
            instant: mode == 1,
        },
        mode == 2,
    ))
}

fn clear_transfer_form(app: &AppWindow) {
    let st = app.global::<AppState>();
    st.set_tf_recipient(0);
    st.set_tf_save_recipient(false);
    st.set_tf_iban_error(false);
    st.set_tf_name(SharedString::default());
    st.set_tf_iban(SharedString::default());
    st.set_tf_bic(SharedString::default());
    st.set_tf_amount(SharedString::default());
    st.set_tf_purpose(SharedString::default());
    st.set_tf_iban_hint(SharedString::default());
    st.set_tf_date(SharedString::default());
}
