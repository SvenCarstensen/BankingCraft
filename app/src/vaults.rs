//! Vault selection contains file paths only. Passwords and bank data stay in the vault.
use crate::{AppState, AppWindow, ctx, store::{Vault, VaultFile}};
use serde::{Deserialize, Serialize};
use crate::ui::{ModelRc, VecModel};
use std::{cell::RefCell, io::Read, path::{Path, PathBuf}, rc::Rc, sync::atomic::Ordering};

#[derive(Default, Serialize, Deserialize)]
struct RecentVaults {
    #[serde(default)]
    paths: Vec<PathBuf>,
}

impl RecentVaults {
    fn load(path: &Path, legacy: &Path) -> Self {
        let read = || -> Option<Self> {
            let file = std::fs::File::open(path).ok()?;
            if file.metadata().ok()?.len() > 65_536 { return None; }
            let mut bytes = Vec::new();
            file.take(65_537).read_to_end(&mut bytes).ok()?;
            if bytes.len() > 65_536 { return None; }
            serde_json::from_slice(&bytes).ok()
        };
        let mut recent = read().unwrap_or_default();
        recent.paths.retain(|p| p.is_absolute());
        for path in &mut recent.paths { if let Ok(canonical) = path.canonicalize() { *path = canonical; } }
        let legacy = legacy.canonicalize().unwrap_or_else(|_| legacy.to_path_buf());
        recent.paths.truncate(20);
        let mut seen = std::collections::HashSet::new();
        recent.paths.retain(|p| seen.insert(p.clone()));
        if VaultFile::exists(&legacy) && !recent.paths.contains(&legacy) {
            recent.paths.push(legacy.to_path_buf());
        }
        recent
    }

    fn remember(&mut self, path: &Path, settings: &Path) -> std::io::Result<()> {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.paths.retain(|p| p != &path);
        self.paths.insert(0, path);
        self.paths.truncate(20);
        if let Some(parent) = settings.parent() { std::fs::create_dir_all(parent)?; }
        crate::private_file::replace(settings, &serde_json::to_vec(self)?)
    }
}

struct Selection {
    recent: RecentVaults,
    settings: PathBuf,
    path: PathBuf,
    creating: bool,
}

impl Selection {
    fn offers_demo(&self) -> bool {
        self.creating && self.recent.paths.is_empty()
    }
}

type Manager = Rc<RefCell<Selection>>;

fn title(path: &Path) -> String {
    path.file_stem().unwrap_or(path.as_os_str()).to_string_lossy().into_owned()
}

fn show_selection(app: &AppWindow, selection: &Selection) {
    let st = app.global::<AppState>();
    let offer_demo = selection.offers_demo() && !st.get_unlocked();
    // Preserve an explicit opt-out while choosing the first vault's filename.
    if !st.get_offer_demo_on_create() || !offer_demo {
        st.set_create_with_demo(offer_demo);
    }
    st.set_offer_demo_on_create(offer_demo);
    let mut names = vec![i18n::tr("Tresor auswählen …").into()];
    names.extend(selection.recent.paths.iter().map(|p| {
        format!("{} · {}", title(p), p.parent().unwrap_or(p).display())
    }));
    st.set_recent_vaults(ModelRc::new(VecModel::from(names)));
    st.set_selected_vault(selection.recent.paths.iter().position(|p| p == &selection.path).map(|i| i as i32 + 1).unwrap_or(0));
    st.set_selected_vault_name(title(&selection.path));
    st.set_selected_vault_path(selection.path.display().to_string());
    st.set_first_run(selection.creating);
    st.set_unlock_error(Default::default());
    // Clear both password inputs whenever the selected file changes.
    st.set_vault_selection_revision(st.get_vault_selection_revision().wrapping_add(1));
}

fn can_switch(app: &AppWindow) -> bool {
    let st = app.global::<AppState>();
    !st.get_busy() && !ctx().running.load(Ordering::Acquire)
        && !st.get_show_confirm() && !st.get_show_pin() && !st.get_show_tan()
        && !st.get_show_pinpad() && !st.get_show_choice()
}

fn select_path(app: &AppWindow, manager: &Manager, path: PathBuf, creating: bool) -> Result<(), String> {
    if !can_switch(app) { return Err(i18n::tr("Bitte den laufenden Vorgang abschließen, bevor Sie den Tresor wechseln.").into()); }
    let path = if creating {
        if std::fs::symlink_metadata(&path).is_ok() {
            return Err(i18n::tr("Diese Datei existiert bereits. Bitte einen anderen Namen für den neuen Tresor wählen.").into());
        }
        path
    } else {
        let path = path.canonicalize().map_err(|e| i18n::tr_format!("Tresor nicht lesbar: {e}"))?;
        if !path.is_file() { return Err(i18n::tr("Tresordatei ist ungültig oder zu groß").into()); }
        path
    };
    let mut selection = manager.borrow_mut();
    selection.path = path;
    selection.creating = creating;
    show_selection(app, &selection);
    app.global::<AppState>().set_show_vault_manager(true);
    Ok(())
}

fn dialog(app: &AppWindow, selection: &Selection) -> rfd::FileDialog {
    let mut dialog = rfd::FileDialog::new()
        .add_filter("BankingCraft", &["kontor"])
        .add_filter(i18n::tr("Alle Dateien"), &["*"]);
    if let Some(parent) = selection.path.parent() { dialog = dialog.set_directory(parent); }
    app.with_native(|window| { dialog=dialog.clone().set_parent(window); });
    dialog
}

fn save_current(app: &AppWindow) -> Result<(), String> {
    if !can_switch(app) { return Err(i18n::tr("Bitte den laufenden Vorgang abschließen, bevor Sie den Tresor wechseln.").into()); }
    if app.global::<AppState>().get_unlocked() { ctx().save()?; }
    Ok(())
}

fn activate(app: &AppWindow, manager: &Manager, vault: Vault, file: VaultFile) {
    reset_vault_view(app);
    let path = file.path().to_path_buf();
    crate::after_unlock(app, vault, file);
    let st = app.global::<AppState>();
    st.set_active_vault_name(title(&path));
    st.set_show_vault_manager(false);
    let mut selection = manager.borrow_mut();
    selection.path = path.clone();
    selection.creating = false;
    let settings = selection.settings.clone();
    let result = selection.recent.remember(&path, &settings);
    show_selection(app, &selection);
    if let Err(e) = result {
        crate::toast(app, &i18n::tr_format!("Tresor geöffnet. Die Auswahl konnte nicht gespeichert werden: {e}"), true);
    }
}

pub fn install(app: &AppWindow) {
    let legacy = crate::store::default_path();
    install_at(app, legacy.with_file_name("vaults.json"), legacy);
}

fn install_at(app: &AppWindow, settings: PathBuf, legacy: PathBuf) -> Manager {
    let recent = RecentVaults::load(&settings, &legacy);
    let path = recent.paths.iter().find(|p| p.is_file()).cloned().unwrap_or(legacy);
    let manager = Rc::new(RefCell::new(Selection { creating: !path.exists(), path, settings, recent }));
    show_selection(app, &manager.borrow());
    let st = app.global::<AppState>();
    let weak = app.as_weak(); let selection = manager.clone();
    st.on_manage_vaults(move || {
        let app = weak.unwrap();
        if !can_switch(&app) { return; }
        if let Some(file) = ctx().file.lock().unwrap().as_ref() {
            let mut selection = selection.borrow_mut();
            selection.path = file.path().to_path_buf(); selection.creating = false;
            show_selection(&app, &selection);
        }
        app.global::<AppState>().set_show_vault_manager(true);
    });
    let weak = app.as_weak(); let selection = manager.clone();
    st.on_cancel_vault_selection(move || {
        let app = weak.unwrap();
        if !app.global::<AppState>().get_unlocked() { return; }
        if let Some(file) = ctx().file.lock().unwrap().as_ref() {
            let mut selection = selection.borrow_mut();
            selection.path = file.path().to_path_buf(); selection.creating = false;
            show_selection(&app, &selection);
        }
        app.global::<AppState>().set_show_vault_manager(false);
    });
    let weak = app.as_weak(); let selection = manager.clone();
    st.on_choose_vault(move |index| {
        let app = weak.unwrap();
        let path = index.checked_sub(1).and_then(|i| usize::try_from(i).ok()).and_then(|i| selection.borrow().recent.paths.get(i).cloned());
        if let Some(path) = path {
            if let Err(e) = select_path(&app, &selection, path, false) {
                show_selection(&app, &selection.borrow());
                app.global::<AppState>().set_unlock_error(e);
            }
        } else { show_selection(&app, &selection.borrow()); }
    });
    let weak = app.as_weak(); let selection = manager.clone();
    st.on_open_vault_file(move || {
        let app = weak.unwrap();
        if !can_switch(&app) { return; }
        let picker = dialog(&app, &selection.borrow()).set_title(i18n::tr("Tresor öffnen"));
        if let Some(path) = picker.pick_file() {
            if let Err(e) = select_path(&app, &selection, path, false) { app.global::<AppState>().set_unlock_error(e); }
        }
    });
    let weak = app.as_weak(); let selection = manager.clone();
    st.on_new_vault_file(move || {
        let app = weak.unwrap();
        if !can_switch(&app) { return; }
        let picker = dialog(&app, &selection.borrow()).set_title(i18n::tr("Neuen Tresor anlegen"))
            .set_file_name(format!("{}.kontor", i18n::tr("Neuer Tresor")));
        if let Some(mut path) = picker.save_file() {
            if path.extension().is_none() { path.set_extension("kontor"); }
            if let Err(e) = select_path(&app, &selection, path, true) { app.global::<AppState>().set_unlock_error(e); }
        }
    });
    let weak = app.as_weak(); let selection = manager.clone();
    st.on_unlock(move |password| {
        let app = weak.unwrap();
        if !can_switch(&app) || selection.borrow().creating { return; }
        let password = zeroize::Zeroizing::new(password);
        let result = {
            let c = ctx(); let file = c.file.lock().unwrap();
            match file.as_ref().filter(|f| f.path() == selection.borrow().path) {
                Some(file) => file.reopen_session(&password),
                None => VaultFile::open_session(selection.borrow().path.clone(), &password),
            }
        }
            .and_then(|(file, vault)| { save_current(&app)?; Ok((file, vault)) });
        match result {
            Ok((file, vault)) => activate(&app, &selection, vault, file),
            Err(e) => app.global::<AppState>().set_unlock_error(e),
        }
    });
    let weak = app.as_weak(); let selection = manager.clone();
    st.on_create_vault(move |password, repeat| {
        let password = zeroize::Zeroizing::new(password);
        let repeat = zeroize::Zeroizing::new(repeat);
        let app = weak.unwrap();
        let st = app.global::<AppState>();
        if !can_switch(&app) || !selection.borrow().creating { return; }
        if password.chars().count() < 8 {
            st.set_unlock_error(i18n::tr("Bitte mindestens 8 Zeichen verwenden.").into()); return;
        }
        if password != repeat {
            st.set_unlock_error(i18n::tr("Die Passwörter stimmen nicht überein.").into()); return;
        }
        let mut vault = Vault { product_version: env!("CARGO_PKG_VERSION").into(), ..Default::default() };
        if selection.borrow().offers_demo() && !st.get_unlocked() && st.get_create_with_demo() {
            crate::demo::add_demo_account(&mut vault);
        }
        let result = save_current(&app).and_then(|_| VaultFile::create_new(selection.borrow().path.clone(), &password, &vault));
        match result {
            Ok(file) => activate(&app, &selection, vault, file),
            Err(e) => st.set_unlock_error(e),
        }
    });
    manager
}

/// Never carry a payment, bank-access draft, authorization or selected transaction
/// from one vault into another. The caller has already checked that no job is running.
pub(crate) fn reset_vault_view(app: &AppWindow) {
    let c = ctx();
    c.bridge.answer(crate::bridge::Answer::Cancel);
    *c.bridge.pin_cache.lock().unwrap() = None;
    *c.bridge.pin_save_requested.lock().unwrap() = false;
    *c.bridge.pin_save_offer.lock().unwrap() = false;
    *c.bridge.usb_payload.lock().unwrap() = None;
    crate::bridge::stop_flicker();
    crate::CONFIRM.with(|pending| *pending.borrow_mut() = None);
    crate::CONFIRM_CANCEL.with(|pending| *pending.borrow_mut() = None);
    crate::BANK_HITS.with(|hits| hits.borrow_mut().clear());
    crate::AUTO_NAME.with(|name| name.borrow_mut().clear());
    crate::VIEW.with(|state| *state.borrow_mut() = Default::default());
    let st = app.global::<AppState>();
    crate::payee_ui::clear(&st);
    st.set_page(0);
    st.set_tx_period(1);
    st.set_so_day("1".into());
    st.set_so_first(chrono::Local::now().date_naive().format("%d.%m.%Y").to_string());
    st.set_busy_text(Default::default());
    st.set_toast(Default::default());
    st.set_last_sync(Default::default());
    st.set_total_balance(Default::default());
    st.set_total_booked_balance(Default::default());
    st.set_total_balance_note(Default::default());
    st.set_total_negative(Default::default());
    st.set_month_label(Default::default());
    st.set_month_income(Default::default());
    st.set_month_expense(Default::default());
    st.set_accounts(Default::default());
    st.set_account_groups(Default::default());
    st.set_group_names(Default::default());
    st.set_new_group_name(Default::default());
    st.set_account_names(Default::default());
    st.set_recent(Default::default());
    st.set_category_stats(Default::default());
    st.set_months(Default::default());
    st.set_upcoming(Default::default());
    st.set_overview_group_names(Default::default());
    st.set_recent_group(0);
    st.set_category_group(0);
    st.set_months_group(0);
    st.set_upcoming_group(0);
    st.set_show_account_name(Default::default());
    st.set_editing_account_id(Default::default());
    st.set_account_name_draft(Default::default());
    st.set_account_bank_name(Default::default());
    st.set_account_name_context(Default::default());
    st.set_tx_rows(Default::default());
    st.set_tx_account(Default::default());
    st.set_tx_search(Default::default());
    st.set_tx_category(Default::default());
    st.set_tx_summary(Default::default());
    st.set_tx_detail(Default::default());
    st.set_category_manager(Default::default());
    st.set_categories(Default::default());
    st.set_category_labels(Default::default());
    st.set_category_colors(Default::default());
    st.set_sepa_accounts(Default::default());
    st.set_tf_account(Default::default());
    st.set_payment_orders(Default::default());
    st.set_queued_order_count(Default::default());
    st.set_recipient_names(Default::default());
    st.set_tf_recipient(Default::default());
    st.set_tf_save_recipient(Default::default());
    st.set_tf_name(Default::default());
    st.set_tf_iban(Default::default());
    st.set_tf_bic(Default::default());
    st.set_tf_amount(Default::default());
    st.set_tf_purpose(Default::default());
    st.set_tf_mode(Default::default());
    st.set_tf_date(Default::default());
    st.set_tf_iban_hint(Default::default());
    st.set_tf_iban_error(Default::default());
    st.set_so_interval(Default::default());
    st.set_so_last(Default::default());
    st.set_standing(Default::default());
    st.set_analysis_categories(Default::default());
    st.set_analysis_income(Default::default());
    st.set_analysis_months(Default::default());
    st.set_analysis_label(Default::default());
    st.set_analysis_offset(Default::default());
    st.set_accesses(Default::default());
    st.set_af_open(Default::default());
    st.set_af_kind(Default::default());
    st.set_af_name(Default::default());
    st.set_af_blz(Default::default());
    st.set_af_user(Default::default());
    st.set_af_customer(Default::default());
    st.set_af_paypal_password(Default::default());
    st.set_af_paypal_signature(Default::default());
    st.set_af_paypal_sandbox(false);
    st.set_total_has_paypal(false);
    st.set_has_foreign_currency(false);
    st.set_af_url(Default::default());
    st.set_af_reader(Default::default());
    st.set_bank_query(Default::default());
    st.set_bank_results(Default::default());
    st.set_bank_picked(Default::default());
    st.set_reader_report(Default::default());
    st.set_product_id(Default::default());
    st.set_product_version(Default::default());
    st.set_data_path(Default::default());
    st.set_old_password(Default::default());
    st.set_new_password(Default::default());
    st.set_show_pin(Default::default());
    st.set_pin_title(Default::default());
    st.set_pin_value(Default::default());
    st.set_pin_offer_save(Default::default());
    st.set_pin_save(Default::default());
    st.set_paste_note(Default::default());
    st.set_show_pinpad(Default::default());
    st.set_pinpad_text(Default::default());
    st.set_show_tan(Default::default());
    st.set_tan_title(Default::default());
    st.set_tan_text(Default::default());
    st.set_tan_kind(Default::default());
    st.set_tan_value(Default::default());
    st.set_tan_image(Default::default());
    st.set_tan_usb_possible(Default::default());
    st.set_show_choice(Default::default());
    st.set_choice_title(Default::default());
    st.set_choices(Default::default());
    st.set_show_confirm(Default::default());
    st.set_confirm_title(Default::default());
    st.set_confirm_text(Default::default());
    st.set_confirm_danger(Default::default());
    st.set_confirm_ok_text(Default::default());
    st.set_af_record("1".into());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bridge::Bridge, jobs::Ctx};
    use crate::ui::Model;
    use std::sync::{Arc, Mutex, atomic::AtomicBool};

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("bankingcraft-vaults-{:016x}", rand::random::<u64>()));
            std::fs::create_dir(&path).unwrap(); Self(path.canonicalize().unwrap())
        }
    }
    impl Drop for Temp { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }

    #[test]
    fn first_vault_offers_demo_by_default_and_saves_the_checkbox_choice() {
        use egui_kittest::kittest::Queryable;
        i18n::with_language(i18n::Language::German, || {
            for include_demo in [true, false] {
                let dir = Temp::new();
                let initial = dir.0.join("vault.kontor");
                let chosen = dir.0.join("private.kontor");
                let app = AppWindow::new().unwrap();
                let c = Ctx { vault: Arc::new(Mutex::new(Vault::default())), file: Arc::new(Mutex::new(None)),
                    bridge: Bridge::new(app.as_weak()), running: Arc::new(AtomicBool::new(false)) };
                crate::CTX.with(|context| *context.borrow_mut() = Some(c.clone()));
                let manager = install_at(&app, dir.0.join("vaults.json"), initial.clone());
                let st = app.global::<AppState>();
                assert!(st.get_first_run() && st.get_offer_demo_on_create() && st.get_create_with_demo());
                let mut harness = crate::ui::testing::harness(&app, 1100., 680.);
                let checkbox = harness.get_by_label("Demokonto mit Beispielumsätzen hinzufügen");
                assert!(checkbox.rect().bottom() < 660.);
                if !include_demo { checkbox.click(); harness.run_steps(3); }
                assert_eq!(st.get_create_with_demo(), include_demo);
                // Choosing a different filename and retrying invalid passwords retain the choice.
                select_path(&app, &manager, chosen.clone(), true).unwrap();
                assert_eq!(st.get_create_with_demo(), include_demo);
                st.invoke_create_vault("short".into(), "short".into());
                st.invoke_create_vault("test-password".into(), "other-password".into());
                assert!(!chosen.exists() && !initial.exists());
                assert!(c.vault.lock().unwrap().accounts.is_empty());
                assert_eq!(st.get_create_with_demo(), include_demo);
                harness.run_steps(3);
                harness.state_mut().passwords = ["test-password".into(), "test-password".into()];
                harness.run_steps(3);
                assert!(harness.get_by_label("Tresor anlegen").rect().bottom() <= 660.);
                harness.get_by_label("Tresor anlegen").click(); harness.run_steps(3);
                assert!(st.get_unlocked(), "{}", st.get_unlock_error());
                assert!(!st.get_offer_demo_on_create() && !st.get_create_with_demo());
                let saved = VaultFile::open(chosen.clone(), "test-password").unwrap().1;
                assert_eq!(saved.accounts.len(), usize::from(include_demo));
                assert_eq!(saved.accesses.len(), usize::from(include_demo));
                assert_eq!(saved.transactions.is_empty(), !include_demo);
                assert!(saved.standing.is_empty());
                if include_demo {
                    assert_eq!(saved.accesses[0].kind, crate::store::AccessKind::Demo);
                    assert!(saved.accounts[0].balance.is_some());
                    assert!(saved.transactions.iter().all(|t| t.account_id == saved.accounts[0].id));
                }
                assert_eq!(st.get_accounts().row_count(), usize::from(include_demo));
                assert!(VaultFile::open(chosen, "wrong-password").is_err());

                let second = dir.0.join("second.kontor");
                select_path(&app, &manager, second.clone(), true).unwrap(); harness.run_steps(3);
                assert!(!st.get_offer_demo_on_create());
                assert!(harness.query_by_label("Demokonto mit Beispielumsätzen hinzufügen").is_none());
                st.set_create_with_demo(true); // A stale UI flag cannot seed subsequent vaults.
                st.invoke_create_vault("second-password".into(), "second-password".into());
                assert!(VaultFile::open(second, "second-password").unwrap().1.accounts.is_empty());
                crate::CTX.with(|context| *context.borrow_mut() = None);
            }
        });
    }

    #[test]
    fn new_vault_is_encrypted_and_cannot_overwrite_an_existing_file_or_link() {
        let dir = Temp::new(); let path = dir.0.join("new.kontor");
        VaultFile::create_new(path.clone(), "first-password", &Vault::default()).unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(VaultFile::create_new(path.clone(), "second-password", &Vault::default()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(VaultFile::open(path.clone(), "first-password").unwrap().1.accounts.is_empty());
        assert!(VaultFile::open(path.clone(), "second-password").is_err());
        #[cfg(unix)] {
            let link = dir.0.join("link.kontor");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(VaultFile::create_new(link, "third-password", &Vault::default()).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn recent_files_preserve_legacy_vault_and_restore_last_successful_choice() {
        let dir = Temp::new(); let legacy = dir.0.join("vault.kontor");
        let settings = dir.0.join("vaults.json");
        std::fs::write(&legacy, "legacy fixture").unwrap();
        let mut recent = RecentVaults::load(&settings, &legacy);
        assert_eq!(recent.paths, vec![legacy.clone()]);
        let second = dir.0.join("business.kontor");
        recent.remember(&second, &settings).unwrap();
        recent.remember(&legacy, &settings).unwrap();
        assert_eq!(RecentVaults::load(&settings, &legacy).paths, vec![legacy.clone(), second]);
        for invalid in ["broken".to_string(), " ".repeat(65_537), "{\"paths\":[\"relative.kontor\"]}".into()] {
            std::fs::write(&settings, invalid).unwrap();
            assert_eq!(RecentVaults::load(&settings, &legacy).paths, vec![legacy.clone()]);
        }
    }

    #[test]
    fn create_open_cancel_and_switch_keep_vaults_and_drafts_separate() {

        let dir = Temp::new(); let first = dir.0.join("private.kontor"); let second = dir.0.join("business.kontor");
        let settings = dir.0.join("vaults.json");
        let mut original = Vault::default(); crate::demo::add_demo(&mut original);
        VaultFile::create_new(first.clone(), "first-password", &original).unwrap();
        let app = AppWindow::new().unwrap();
        let c = Ctx { vault: Arc::new(Mutex::new(Vault::default())), file: Arc::new(Mutex::new(None)),
            bridge: Bridge::new(app.as_weak()), running: Arc::new(AtomicBool::new(false)) };
        crate::CTX.with(|context| *context.borrow_mut() = Some(c.clone()));
        let manager = install_at(&app, settings.clone(), first.clone());
        let st = app.global::<AppState>();
        assert!(!st.get_first_run());
        assert_eq!(st.get_recent_vaults().row_count(), 2);
        st.invoke_unlock("incorrect-password".into());
        assert!(!st.get_unlocked()); assert!(c.file.lock().unwrap().is_none());
        st.invoke_unlock("first-password".into());
        assert!(st.get_unlocked()); assert_eq!(st.get_active_vault_name(), "private");
        assert_eq!(st.get_accounts().row_count(), 2);
        st.set_tf_name("private unsaved draft".into());
        st.set_af_user("private login draft".into());
        st.set_af_paypal_password("synthetic API password".into());
        st.set_af_paypal_signature("synthetic API signature".into());
        st.set_account_name_draft("private alias draft".into());
        crate::view::show_detail(&app, &original, 0);
        *c.bridge.pin_cache.lock().unwrap() = Some(zeroize::Zeroizing::new("private pin".into()));
        let revision = st.get_vault_selection_revision();
        select_path(&app, &manager, second.clone(), true).unwrap();
        assert!(st.get_vault_selection_revision() > revision);
        assert!(st.get_unlocked()); assert!(st.get_show_vault_manager());
        st.invoke_cancel_vault_selection();
        assert_eq!(st.get_tf_name(), "private unsaved draft");
        assert_eq!(manager.borrow().path, first);
        assert!(!st.get_show_vault_manager());
        c.running.store(true, Ordering::Release);
        assert!(select_path(&app, &manager, second.clone(), true).is_err());
        c.running.store(false, Ordering::Release);
        st.set_busy(true);
        assert!(select_path(&app, &manager, second.clone(), true).is_err());
        st.set_busy(false);
        select_path(&app, &manager, second.clone(), true).unwrap();
        st.invoke_create_vault("short".into(), "short".into()); assert!(!second.exists());
        st.invoke_create_vault("second-password".into(), "different-password".into()); assert!(!second.exists());
        st.invoke_create_vault("second-password".into(), "second-password".into());
        assert_eq!(st.get_active_vault_name(), "business");
        assert_eq!(st.get_accounts().row_count(), 0);
        assert_eq!(st.get_payment_orders().row_count(), 0);
        assert!(st.get_tf_name().is_empty()); assert!(st.get_af_user().is_empty());
        assert!(st.get_af_paypal_password().is_empty()); assert!(st.get_af_paypal_signature().is_empty());
        assert!(st.get_account_name_draft().is_empty()); assert!(!st.get_tx_detail().valid);
        assert!(c.bridge.pin_cache.lock().unwrap().is_none());
        assert_eq!(VaultFile::open(first.clone(), "first-password").unwrap().1.accounts.len(), 2);
        assert!(VaultFile::open(second.clone(), "second-password").unwrap().1.accounts.is_empty());
        assert!(VaultFile::open(second.clone(), "first-password").is_err());
        c.edit_vault(|v| { v.merge_accounts(1, vec![banking::Account { product: "Only business".into(), account_number: "123".into(), ..Default::default() }]); Ok(()) }).unwrap();
        crate::refresh_view(&app);
        st.set_tf_name("business draft".into());
        st.invoke_choose_vault(2);
        assert_eq!(manager.borrow().path, first);
        st.invoke_unlock("second-password".into());
        assert_eq!(st.get_active_vault_name(), "business");
        assert_eq!(st.get_accounts().row_data(0).unwrap().name, "Only business");
        assert_eq!(st.get_tf_name(), "business draft");
        st.invoke_unlock("first-password".into());
        assert_eq!(st.get_active_vault_name(), "private");
        assert_eq!(st.get_accounts().row_count(), 2); assert!(st.get_tf_name().is_empty());
        assert_eq!(RecentVaults::load(&settings, &first).paths, vec![first.clone(), second.clone()]);
        assert!(select_path(&app, &manager, first.clone(), true).is_err());
        assert!(select_path(&app, &manager, dir.0.join("missing.kontor"), false).is_err());
        // If saving the current vault fails, a successfully decrypted second vault
        // must still not replace the current session or its forms.
        let blocker = dir.0.join("not-a-folder"); std::fs::write(&blocker, "keep").unwrap();
        let old_file = c.file.lock().unwrap().replace(VaultFile::create(blocker.join("vault.kontor"), "first-password").unwrap());
        select_path(&app, &manager, second, false).unwrap();
        st.invoke_unlock("second-password".into());
        assert_eq!(st.get_active_vault_name(), "private"); assert_eq!(st.get_accounts().row_count(), 2);
        assert!(!st.get_unlock_error().is_empty());
        *c.file.lock().unwrap() = old_file;
        crate::CTX.with(|context| *context.borrow_mut() = None);
    }
}
