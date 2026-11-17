use crate::{AppState, AppWindow, CategoryManager, ctx, refresh_view, store::Vault, view};
use std::sync::atomic::Ordering;

fn apply<T>(app: &AppWindow, action: &'static str, edit: impl FnOnce(&mut Vault) -> Result<T, String>) -> Option<T> {
    let st = app.global::<AppState>();
    let c = ctx();
    if !st.get_unlocked() || st.get_busy() || c.running.load(Ordering::Acquire) { return None; }
    match c.edit_vault_logged(action, edit) {
        Ok(result) => Some(result),
        Err(error) => {
            let mut manager = st.get_category_manager();
            manager.error = error;
            st.set_category_manager(manager);
            None
        }
    }
}

fn refreshed(app: &AppWindow, original: &str, replacement: &str) {
    let st = app.global::<AppState>();
    if !original.is_empty() && st.get_tx_category() == original {
        st.set_tx_category(replacement.into());
    }
    refresh_view(app);
    let detail = st.get_tx_detail();
    if detail.valid { view::show_detail(app, &ctx().vault.lock().unwrap(), detail.index as usize); }
    st.set_category_manager(CategoryManager::default());
}

pub fn install(app: &AppWindow) {
    let st = app.global::<AppState>();
    let weak = app.as_weak();
    st.on_save_category(move || {
        let app = weak.unwrap();
        let manager = app.global::<AppState>().get_category_manager();
        if app.global::<AppState>().get_page() != 10 || manager.deleting { return; }
        if let Some(category) = apply(&app, "Kategorie gespeichert", |v| if manager.original.is_empty() {
            v.create_category(&manager.name).map(|()| manager.name.trim().to_owned())
        } else { v.rename_category(&manager.original, &manager.name) }) {
            refreshed(&app, &manager.original, &category);
        }
    });
    let weak = app.as_weak();
    st.on_delete_category(move || {
        let app = weak.unwrap();
        let manager = app.global::<AppState>().get_category_manager();
        if app.global::<AppState>().get_page() != 10 || !manager.deleting { return; }
        if apply(&app, "Kategorie gelöscht", |v| v.delete_category(&manager.original, &manager.replacement)).is_some() {
            refreshed(&app, &manager.original, &manager.replacement);
        }
    });
    let weak = app.as_weak();
    st.on_set_category(move |index, category| {
        let app = weak.unwrap();
        let st = app.global::<AppState>();
        if !st.get_unlocked() || st.get_busy() { return; }
        if crate::edit_vault(&app, "Umsatz kategorisiert", |v| v.assign_category(index as usize, &category)) {
            view::show_detail(&app, &ctx().vault.lock().unwrap(), index as usize);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bridge::Bridge, jobs::Ctx, store::VaultFile, ui::Model};
    use egui_kittest::kittest::{By, Queryable};
    use std::sync::{Arc, Mutex, atomic::AtomicBool};

    struct Fixture { app: AppWindow, c: Ctx, path: std::path::PathBuf }
    impl Fixture {
        fn new() -> Self {
            let app = AppWindow::new().unwrap();
            let path = std::env::temp_dir().join(format!("bankingcraft-categories-{:016x}", rand::random::<u64>()));
            let mut vault = Vault::default();
            let account = vault.merge_accounts(1, vec![banking::Account { account_number: "123".into(), ..Default::default() }])[0];
            vault.merge_transactions(account, vec![crate::business_tests::tx(-100, "2026-11-01"), crate::business_tests::tx(-200, "2026-11-02")]);
            let c = Ctx { vault: Arc::new(Mutex::new(vault)),
                file: Arc::new(Mutex::new(Some(VaultFile::create(path.clone(), "test").unwrap()))),
                bridge: Bridge::new(app.as_weak()), running: Arc::new(AtomicBool::new(false)) };
            crate::CTX.with(|ctx| *ctx.borrow_mut() = Some(c.clone()));
            crate::VIEW.with(|vs| *vs.borrow_mut() = Default::default());
            install(&app);
            let weak = app.as_weak();
            app.global::<AppState>().on_navigate(move |page| weak.unwrap().global::<AppState>().set_page(page));
            let st = app.global::<AppState>();
            st.set_unlocked(true); st.set_page(8); st.set_tx_period(3);
            refresh_view(&app);
            Self { app, c, path }
        }
        fn saved(&self) -> Vault { VaultFile::open(self.path.clone(), "test").unwrap().1 }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            crate::CTX.with(|ctx| *ctx.borrow_mut() = None);
            crate::VIEW.with(|vs| *vs.borrow_mut() = Default::default());
            let _ = std::fs::remove_file(&self.path);
        }
    }

    #[test]
    fn create_rename_delete_through_ui_updates_filters_details_rules_and_encrypted_storage() {
        i18n::with_language(i18n::Language::German, || {
            let f = Fixture::new();
            let st = f.app.global::<AppState>();
            let mut h = crate::ui::testing::harness(&f.app, 1100., 800.);
            h.get_by_label("Kategorien verwalten").click(); h.run_steps(3);
            h.get(By::new().role(egui::accesskit::Role::TextInput)).type_text("Urlaub"); h.run_steps(2);
            h.key_press(egui::Key::Enter); h.run_steps(3);
            assert_eq!(f.saved().custom_category_names(), ["Urlaub"]);
            assert!(st.get_categories().iter().any(|c| c == "Urlaub"));
            let stored = std::fs::read(&f.path).unwrap();
            assert!(!stored.windows(6).any(|w| w == b"Urlaub"));

            st.invoke_set_category(0, "Urlaub".into());
            st.set_tx_category("Urlaub".into());
            refresh_view(&f.app);
            assert_eq!(st.get_tx_rows().iter().filter(|r| !r.is_header).count(), 2);
            h.run_steps(3);
            h.get_by_label("Kategorie umbenennen: Urlaub").click(); h.run_steps(3);
            let mut manager = st.get_category_manager(); manager.name = "Reisen".into(); st.set_category_manager(manager);
            h.run_steps(3);
            h.get_by_label("Umbenennen").click(); h.run_steps(3);
            assert_eq!(f.saved().custom_category_names(), ["Reisen"]);
            assert_eq!(st.get_tx_category(), "Reisen");
            assert_eq!(st.get_tx_detail().category, "Reisen");
            assert!(st.get_tx_rows().iter().filter(|r| !r.is_header).all(|r| r.category == "Reisen"));
            assert!(st.get_analysis_categories().iter().any(|c| c.label == "Reisen"));

            h.get_by_label("Kategorie löschen: Reisen").click(); h.run_steps(3);
            assert!(st.get_category_manager().deleting);
            h.get_by_label("Abbrechen").click(); h.run_steps(3);
            assert_eq!(f.saved().custom_category_names(), ["Reisen"]);
            h.get_by_label("Kategorie löschen: Reisen").click(); h.run_steps(3);
            h.get_by_label("Kategorie löschen").click(); h.run_steps(3);
            let saved = f.saved();
            assert!(saved.custom_category_names().is_empty());
            assert_eq!(saved.transactions[0].category.as_deref(), Some("Sonstiges"));
            assert_eq!(saved.rules[0].category, "Sonstiges");
            assert_eq!(st.get_tx_category(), "Sonstiges");
            assert_eq!(st.get_tx_detail().category, "Sonstiges");
            assert!(!st.get_categories().iter().any(|c| c == "Reisen"));
            h.get_by_label("Einstellungen").click(); h.run_steps(3);
            assert_eq!(st.get_page(), 8);
        });
    }

    #[test]
    fn failed_save_and_locked_or_busy_vault_leave_categories_and_assignments_unchanged() {
        let f = Fixture::new();
        let st = f.app.global::<AppState>();
        st.set_page(10);
        st.set_category_manager(CategoryManager { name: "Urlaub".into(), ..Default::default() });
        st.invoke_save_category();
        st.invoke_set_category(0, "Urlaub".into());
        let before = serde_json::to_value(&*f.c.vault.lock().unwrap()).unwrap();
        let original = f.c.file.lock().unwrap().take();
        st.set_category_manager(CategoryManager { original: "Urlaub".into(), name: "Reisen".into(), ..Default::default() });
        st.invoke_save_category();
        assert!(!st.get_category_manager().error.is_empty());
        st.invoke_set_category(0, "Wohnen".into());
        assert_eq!(serde_json::to_value(&*f.c.vault.lock().unwrap()).unwrap(), before);
        assert_eq!(serde_json::to_value(f.saved()).unwrap(), before);
        *f.c.file.lock().unwrap() = original;
        st.set_unlocked(false); st.invoke_save_category();
        st.set_unlocked(true); st.set_busy(true); st.invoke_save_category();
        st.set_busy(false); f.c.running.store(true, Ordering::Release); st.invoke_save_category();
        assert_eq!(serde_json::to_value(&*f.c.vault.lock().unwrap()).unwrap(), before);
        f.c.running.store(false, Ordering::Release);
        crate::vaults::reset_vault_view(&f.app);
        assert_eq!(st.get_category_manager(), CategoryManager::default());
    }

    #[test]
    fn custom_names_are_not_translated_and_category_page_fits_all_languages() {
        for language in [i18n::Language::German, i18n::Language::English, i18n::Language::French, i18n::Language::Spanish] {
            i18n::with_language(language, || {
                let f = Fixture::new();
                f.c.vault.lock().unwrap().create_category("Einstellungen").unwrap();
                f.c.vault.lock().unwrap().create_category("Sommerurlaub mit Familie und Freunden am Mittelmeer").unwrap();
                refresh_view(&f.app);
                let st = f.app.global::<AppState>();
                assert!(st.get_category_labels().iter().any(|c| c == "Einstellungen"));
                for theme in [1, 2] {
                    st.set_theme_mode(theme);
                    st.set_page(10);
                    st.set_category_manager(CategoryManager::default());
                    let mut h = crate::ui::testing::harness(&f.app, 1100., 680.);
                    let create = h.get_by_label(i18n::tr("Kategorie erstellen")).rect();
                    assert!(create.bottom() <= 680. && create.right() <= 1100.);
                    let bank = h.get_by_label(i18n::tr("Bankzugänge")).rect();
                    let categories = h.get(By::new().role(egui::accesskit::Role::Button).label(i18n::tr("Kategorien"))).rect();
                    let reader = h.get_by_label(i18n::tr("Kartenleser")).rect();
                    assert!(bank.bottom() < categories.top() && categories.bottom() < reader.top());
                    if let Some(dir) = std::env::var_os("BANKINGCRAFT_CATEGORY_REVIEW") {
                        std::fs::create_dir_all(&dir).unwrap();
                        h.render().unwrap().save(std::path::Path::new(&dir).join(format!("{}-{theme}-list.png", language.code()))).unwrap();
                    }
                    st.set_category_manager(CategoryManager { original: "Einstellungen".into(), deleting: true, replacement: "Sonstiges".into(), ..Default::default() });
                    h.run_steps(3);
                    let delete = h.get_by_label(i18n::tr("Kategorie löschen")).rect();
                    assert!(delete.bottom() <= 680. && delete.right() <= 1100.);
                    if let Some(dir) = std::env::var_os("BANKINGCRAFT_CATEGORY_REVIEW") {
                        h.render().unwrap().save(std::path::Path::new(&dir).join(format!("{}-{theme}-delete.png", language.code()))).unwrap();
                    }
                }
            });
        }
    }
    #[test]
    fn builtin_category_page_renames_and_deletes_automatic_and_manual_assignments() {
        i18n::with_language(i18n::Language::German, || {
            let f = Fixture::new();
            f.c.vault.lock().unwrap().transactions[0].tx.counterparty_name = "REWE".into();
            let st = f.app.global::<AppState>();
            st.set_tx_category("Lebensmittel".into());
            st.invoke_set_category(1, "Lebensmittel".into());
            view::show_detail(&f.app, &f.c.vault.lock().unwrap(), 0);
            let mut h = crate::ui::testing::harness(&f.app, 1100., 680.);
            h.get(By::new().role(egui::accesskit::Role::Button).label("Kategorien")).click(); h.run_steps(3);
            assert_eq!(st.get_page(), 10);
            let color = st.get_category_colors().row_data(0);
            h.get_by_label("Kategorie umbenennen: Lebensmittel").click(); h.run_steps(4);
            assert_eq!(st.get_category_manager().name, "Lebensmittel");
            let mut manager = st.get_category_manager(); manager.name = "Einkäufe".into(); st.set_category_manager(manager);
            h.run_steps(3); h.get_by_label("Umbenennen").click(); h.run_steps(4);
            assert_eq!(st.get_tx_category(), "Lebensmittel");
            assert_eq!(st.get_tx_detail().category, "Einkäufe");
            assert_eq!(st.get_tx_detail().category_id, "Lebensmittel");
            assert_eq!(st.get_category_colors().row_data(0), color);
            assert_eq!(f.saved().category_label("Lebensmittel"), "Einkäufe");
            assert!(st.get_analysis_categories().iter().any(|c| c.label == "Einkäufe"));
            assert!(crate::export::csv_content(&f.saved(), &[0, 1]).contains("Einkäufe"));
            h.get_by_label("Kategorie löschen: Einkäufe").click(); h.run_steps(4);
            assert!(st.get_category_manager().deleting);
            h.get_by_label("Kategorie löschen").click(); h.run_steps(4);
            let saved = f.saved();
            assert!(!saved.category_names().iter().any(|c| c == "Lebensmittel"));
            assert_eq!(st.get_tx_category(), "Sonstiges");
            assert_eq!(st.get_tx_detail().category, "Sonstiges");
            assert!(saved.transactions.iter().all(|t| crate::categorize::categorize(t, &saved) == "Sonstiges"));
            assert!(Vault::default().category_overrides.is_empty(), "customizations stay within their vault");
        });
    }

}
