// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

slint::include_modules!();

mod banks;
mod bridge;
mod categorize;
mod demo;
mod export;
mod jobs;
mod store;
mod view;

use bridge::{Answer, Bridge};
use chrono::Local;
use fints::{Interval, SepaTransfer, StandingOrder};
use jobs::Ctx;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use store::{AccessKind, BankAccess, Rule, Vault, VaultFile};

thread_local! {
    static VIEW: RefCell<view::ViewState> = RefCell::new(view::ViewState::default());
    static CTX: RefCell<Option<Ctx>> = const { RefCell::new(None) };
    static CONFIRM: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
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
    slint::Timer::single_shot(std::time::Duration::from_secs(secs), move || {
        if let Some(app) = weak.upgrade() {
            let st = app.global::<AppState>();
            if st.get_toast().text == shown.as_str() {
                st.set_toast(Toast::default());
            }
        }
    });
}

fn confirm(app: &AppWindow, title: &str, text: &str, action: impl FnOnce() + 'static) {
    let st = app.global::<AppState>();
    st.set_confirm_danger(title.contains("löschen") || title.contains("entfernen"));
    st.set_confirm_title(title.into());
    st.set_confirm_text(text.into());
    st.set_show_confirm(true);
    CONFIRM.with(|c| *c.borrow_mut() = Some(Box::new(action)));
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
        st.set_reader_names(ModelRc::new(VecModel::from(vec![SharedString::from("(kein Kartenleser gefunden)")])));
    } else {
        st.set_reader_names(ModelRc::new(VecModel::from(names)));
    }
}

fn selected_reader(app: &AppWindow, idx: i32) -> Option<String> {
    let st = app.global::<AppState>();
    let r = st.get_reader_names().row_data(idx.max(0) as usize)?.to_string();
    if r.starts_with('(') { None } else { Some(r) }
}

fn after_unlock(app: &AppWindow, vault: Vault, file: VaultFile) {
    let st = app.global::<AppState>();
    st.set_data_path(file.path().display().to_string().into());
    st.set_product_id(vault.product_id.clone().into());
    st.set_product_version(if vault.product_version.is_empty() { env!("CARGO_PKG_VERSION").into() } else { vault.product_version.clone().into() });
    let c = ctx();
    *c.vault.lock().unwrap() = vault;
    *c.file.lock().unwrap() = Some(file);
    st.set_unlocked(true);
    st.set_unlock_error(SharedString::default());
    refresh_view(app);
    refresh_readers(app);
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let app = AppWindow::new().expect("UI");
    let st = app.global::<AppState>();
    let path = store::default_path();
    st.set_first_run(!VaultFile::exists(&path));
    st.set_tx_period(1);
    st.set_so_first(Local::now().date_naive().format("%d.%m.%Y").to_string().into());

    let bridge = Bridge::new(app.as_weak());
    CTX.with(|c| {
        *c.borrow_mut() = Some(Ctx { vault: Arc::new(Mutex::new(Vault::default())), file: Arc::new(Mutex::new(None)), bridge: bridge.clone() })
    });

    // ---------------------------------------------------------------- vault
    {
        let weak = app.as_weak();
        let path = path.clone();
        st.on_unlock(move |pw| {
            let app = weak.unwrap();
            match VaultFile::open(path.clone(), &pw) {
                Ok((file, vault)) => after_unlock(&app, vault, file),
                Err(e) => app.global::<AppState>().set_unlock_error(e.into()),
            }
        });
    }
    {
        let weak = app.as_weak();
        let path = path.clone();
        st.on_create_vault(move |p1, p2| {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            if p1.chars().count() < 8 {
                st.set_unlock_error("Bitte mindestens 8 Zeichen verwenden.".into());
                return;
            }
            if p1 != p2 {
                st.set_unlock_error("Die Passwörter stimmen nicht überein.".into());
                return;
            }
            match VaultFile::create(path.clone(), &p1) {
                Ok(file) => {
                    let vault = Vault { product_version: env!("CARGO_PKG_VERSION").into(), ..Default::default() };
                    if let Err(e) = file.save(&vault) {
                        st.set_unlock_error(e.into());
                        return;
                    }
                    st.set_first_run(false);
                    after_unlock(&app, vault, file);
                }
                Err(e) => st.set_unlock_error(e.into()),
            }
        });
    }

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
    st.on_refresh_all(|| ctx().run("Kontenrundruf …", jobs::sync_all));
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
                Ok(p) => toast(&app, &format!("{} Umsätze exportiert:\n{}", idx.len(), p.display()), false),
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
                Ok(p) => toast(&app, &format!("{} Umsätze als camt.052 exportiert:\n{}", idx.len(), p.display()), false),
                Err(e) => toast(&app, &e, true),
            }
        });
    }

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
                st.set_tf_iban_hint("IBAN gültig".into());
                st.set_tf_iban_error(false);
            } else {
                st.set_tf_iban_hint("Prüfziffer passt nicht – bitte IBAN kontrollieren".into());
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
                toast(&app, "Bitte ein Auftraggeberkonto wählen.", true);
                return;
            };
            let (t, scheduled) = match transfer_from_form(&app) {
                Ok(x) => x,
                Err(e) => {
                    toast(&app, &e, true);
                    return;
                }
            };
            let text = format!(
                "{} an {}\n{}\n„{}“{}",
                view::fmt_eur(t.amount),
                t.creditor_name,
                t.creditor_iban,
                t.purpose,
                t.execution_date.map(|d| format!("\nAusführung am {}", view::fmt_date(d))).unwrap_or_default()
            );
            let weak2 = app.as_weak();
            confirm(&app, "Überweisung senden?", &text, move || {
                ctx().run("Überweisung wird übermittelt …", move |c| {
                    let r = jobs::transfer(c, account_id, t, scheduled);
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
                toast(&app, "Bitte ein Auftraggeberkonto wählen.", true);
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
                toast(&app, "Bitte das Datum der ersten Ausführung als TT.MM.JJJJ angeben.", true);
                return;
            };
            let last = view::parse_date(&st.get_so_last());
            let day: u32 = st.get_so_day().trim().parse().unwrap_or(1);
            let (interval, every) = match st.get_so_interval() {
                1 => (Interval::Monthly, 3),
                2 => (Interval::Monthly, 6),
                3 => (Interval::Monthly, 12),
                4 => (Interval::Weekly, 1),
                _ => (Interval::Monthly, 1),
            };
            let so = StandingOrder {
                order_id: String::new(),
                transfer: t,
                first_date: first,
                last_date: last,
                interval,
                every,
                day,
                raw_pain: Vec::new(),
                descriptor: String::new(),
            };
            let text = format!("{} an {}, {}", view::fmt_eur(so.transfer.amount), so.transfer.creditor_name, view::schedule_text(&so));
            confirm(&app, "Dauerauftrag anlegen?", &text, move || {
                ctx().run("Dauerauftrag wird angelegt …", move |c| jobs::create_standing(c, account_id, so));
            });
        });
    }
    st.on_fetch_standing(|| ctx().run("Daueraufträge werden abgerufen …", jobs::fetch_standing));
    {
        let weak = app.as_weak();
        st.on_delete_standing(move |i| {
            let app = weak.unwrap();
            let c = ctx();
            let name = c.vault.lock().unwrap().standing.get(i as usize).map(|s| s.order.transfer.creditor_name.clone()).unwrap_or_default();
            confirm(&app, "Dauerauftrag löschen?", &format!("Der Dauerauftrag an {name} wird bei der Bank gelöscht."), move || {
                ctx().run("Dauerauftrag wird gelöscht …", move |c| jobs::delete_standing(c, i as usize));
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
            let chip = st.get_af_kind() == 1;
            st.set_bank_picked(SharedString::default());
            let hits = banks::search(&q, chip, 8);
            let rows: Vec<BankRow> = hits
                .iter()
                .enumerate()
                .map(|(i, b)| BankRow {
                    index: i as i32,
                    name: b.name.into(),
                    detail: format!("BLZ {} · {} · {}", b.blz, b.city, b.bic).into(),
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
            let chip = st.get_af_kind() == 1;
            st.set_af_blz(b.blz.into());
            st.set_af_url(if chip { b.host } else { b.url }.into());
            let current = st.get_af_name().trim().to_string();
            if current.is_empty() || AUTO_NAME.with(|n| *n.borrow() == current) {
                st.set_af_name(b.name.into());
                AUTO_NAME.with(|n| *n.borrow_mut() = b.name.to_string());
            }
            st.set_bank_query(b.name.into());
            let hint = banks::login_hint(b);
            st.set_bank_picked(format!("✓ {} · BLZ {} übernommen{}", b.name, b.blz, hint.map(|h| format!(" – {h}")).unwrap_or_default()).into());
            st.set_bank_results(ModelRc::new(VecModel::from(Vec::<BankRow>::new())));
        });
    }
    {
        let weak = app.as_weak();
        st.on_read_card(move || {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            let Some(reader) = selected_reader(&app, st.get_af_reader()) else {
                toast(&app, "Kein Kartenleser ausgewählt.", true);
                return;
            };
            let record: u8 = st.get_af_record().trim().parse().unwrap_or(1);
            let weak = app.as_weak();
            st.set_busy(true);
            st.set_busy_text("Lese Bankdaten von der Chipkarte …".into());
            std::thread::spawn(move || {
                let r = jobs::read_card(&reader, record);
                let _ = weak.upgrade_in_event_loop(move |app| {
                    let st = app.global::<AppState>();
                    st.set_busy(false);
                    match r {
                        Ok(b) => {
                            st.set_af_blz(b.blz.clone().into());
                            st.set_af_user(b.user_id.clone().into());
                            let host = if b.host.trim().is_empty() { banks::by_blz(&b.blz).map(|x| x.host.to_string()).unwrap_or_default() } else { b.host.clone() };
                            st.set_af_url(host.into());
                            if st.get_af_name().is_empty() {
                                st.set_af_name(format!("{} (Chipkarte)", b.short_name).into());
                            }
                            toast(&app, &format!("Karte gelesen: {} · BLZ {}", b.short_name, b.blz), false);
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
            let blz: String = st.get_af_blz().chars().filter(|c| c.is_ascii_digit()).collect();
            let user = st.get_af_user().trim().to_string();
            let url = st.get_af_url().trim().to_string();
            let chip = st.get_af_kind() == 1;
            if blz.len() != 8 {
                toast(&app, "Die Bankleitzahl muss 8 Ziffern haben.", true);
                return;
            }
            if user.is_empty() {
                toast(&app, "Bitte die Benutzerkennung angeben.", true);
                return;
            }
            if !chip && !url.starts_with("https://") {
                toast(&app, "Bitte die FinTS-URL Ihrer Bank (https://…) angeben.", true);
                return;
            }
            let kind = if chip {
                let Some(reader) = selected_reader(&app, st.get_af_reader()) else {
                    toast(&app, "Kein Kartenleser ausgewählt.", true);
                    return;
                };
                AccessKind::ChipCard { reader, record: st.get_af_record().trim().parse().unwrap_or(1) }
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
                        url,
                        ..Default::default()
                    },
                    state: fints::State::default(),
                });
                id
            };
            st.set_af_open(false);
            let _ = c.save();
            refresh_view(&app);
            c.run("Bankzugang wird eingerichtet …", move |c| jobs::sync_access(c, id));
        });
    }
    {
        let weak = app.as_weak();
        st.on_delete_access(move |id| {
            let app = weak.unwrap();
            let c = ctx();
            let name = c.vault.lock().unwrap().access(id as u32).map(|a| a.name.clone()).unwrap_or_default();
            let weak = app.as_weak();
            confirm(&app, "Bankzugang entfernen?", &format!("„{name}“ mit allen lokal gespeicherten Konten und Umsätzen entfernen? Bei der Bank wird nichts geändert."), move || {
                let c = ctx();
                c.vault.lock().unwrap().remove_access(id as u32);
                let _ = c.save();
                if let Some(app) = weak.upgrade() {
                    refresh_view(&app);
                }
            });
        });
    }
    st.on_sync_access(|id| ctx().run("Bankzugang wird synchronisiert …", move |c| jobs::sync_access(c, id as u32)));
    st.on_reset_tan(|id| ctx().run("TAN-Verfahren werden abgefragt …", move |c| jobs::reset_tan(c, id as u32)));

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
            st.set_busy_text("Prüfe Kartenleser …".into());
            let weak = app.as_weak();
            std::thread::spawn(move || {
                let report = cardreader::diagnose(reader.as_deref());
                let _ = weak.upgrade_in_event_loop(move |app| {
                    let st = app.global::<AppState>();
                    st.set_busy(false);
                    st.set_reader_report(ModelRc::new(VecModel::from(report.into_iter().map(SharedString::from).collect::<Vec<_>>())));
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
                Ok(()) => toast(&app, "Einstellungen gespeichert.", false),
                Err(e) => toast(&app, &e, true),
            }
        });
    }
    {
        let weak = app.as_weak();
        st.on_change_password(move || {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            let (old, new) = (st.get_old_password().to_string(), st.get_new_password().to_string());
            if new.chars().count() < 8 {
                toast(&app, "Das neue Passwort braucht mindestens 8 Zeichen.", true);
                return;
            }
            let c = ctx();
            let r = c.file.lock().unwrap().as_mut().map(|f| f.change_password(&old, &new)).unwrap_or(Err("Tresor nicht geöffnet".into()));
            match r.and_then(|_| c.save()) {
                Ok(()) => {
                    st.set_old_password(SharedString::default());
                    st.set_new_password(SharedString::default());
                    toast(&app, "Tresor-Passwort geändert.", false);
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
            toast(&app, "Demo-Bankzugang mit Beispielumsätzen angelegt.", false);
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
            st.set_pin_value(SharedString::default());
            st.set_paste_note(SharedString::default());
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
                        st.set_paste_note("Eingefügt.".into());
                        return;
                    }
                    // do not leave the PIN lying around in the clipboard
                    let cleared = clear_clipboard_if(&text);
                    st.set_paste_note(
                        if cleared { "PIN eingefügt – die Zwischenablage wurde aus Sicherheitsgründen geleert." } else { "PIN eingefügt." }.into(),
                    );
                }
                Err(e) => st.set_paste_note(e.into()),
            }
        });
    }
    {
        let weak = app.as_weak();
        st.on_tan_paste(move || {
            let app = weak.unwrap();
            let st = app.global::<AppState>();
            match clipboard_text() {
                Ok(text) => st.set_tan_value(text.into()),
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
            b.pinpad(true, "Bitte prüfen Sie Empfänger und Betrag im Display des Kartenlesers und bestätigen Sie dort mit OK.");
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
        });
    }
    {
        let weak = app.as_weak();
        st.on_toast_close(move || weak.unwrap().global::<AppState>().set_toast(Toast::default()));
    }

    app.run().expect("event loop");
}

/// Text from the system clipboard, trimmed (PINs/TANs are often copied with a trailing newline).
fn clipboard_text() -> Result<String, String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| format!("Zwischenablage nicht verfügbar: {e}"))?;
    let text = cb.get_text().map_err(|_| "Die Zwischenablage enthält keinen Text.".to_string())?;
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("Die Zwischenablage ist leer.".into());
    }
    if text.chars().count() > 64 || text.contains('\n') {
        return Err("Der Inhalt der Zwischenablage sieht nicht nach einer PIN/TAN aus.".into());
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
    let amount = view::parse_amount(&st.get_tf_amount()).ok_or("Bitte einen gültigen Betrag angeben.")?;
    let purpose = st.get_tf_purpose().trim().to_string();
    if name.is_empty() {
        return Err("Bitte den Empfänger angeben.".into());
    }
    if !fints::pain::iban_valid(&iban) {
        return Err("Die IBAN ist ungültig.".into());
    }
    if amount <= rust_decimal::Decimal::ZERO {
        return Err("Der Betrag muss größer als 0 sein.".into());
    }
    if purpose.chars().count() > 140 {
        return Err("Der Verwendungszweck ist länger als 140 Zeichen.".into());
    }
    let mode = st.get_tf_mode();
    let execution_date = if mode == 2 {
        let d = view::parse_date(&st.get_tf_date()).ok_or("Bitte das Ausführungsdatum als TT.MM.JJJJ angeben.")?;
        if d <= Local::now().date_naive() {
            return Err("Eine Terminüberweisung muss in der Zukunft liegen.".into());
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
    st.set_tf_name(SharedString::default());
    st.set_tf_iban(SharedString::default());
    st.set_tf_bic(SharedString::default());
    st.set_tf_amount(SharedString::default());
    st.set_tf_purpose(SharedString::default());
    st.set_tf_iban_hint(SharedString::default());
    st.set_tf_date(SharedString::default());
}
