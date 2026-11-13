//! The UI language is an unencrypted preference; it never changes banking data.
use i18n::Language;
use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{cell::Cell, io::Read, path::Path, rc::Rc};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Mode { #[default] System, De, En, Fr, Es }
impl Mode {
    fn from_index(index: i32) -> Option<Self> {
        [Self::System, Self::De, Self::En, Self::Fr, Self::Es].get(usize::try_from(index).ok()?).copied()
    }
    fn index(self) -> i32 { match self { Self::System => 0, Self::De => 1, Self::En => 2, Self::Fr => 3, Self::Es => 4 } }
    fn resolve(self, system: &str) -> Language {
        match self { Self::System => Language::from_locale(system), Self::De => Language::German, Self::En => Language::English, Self::Fr => Language::French, Self::Es => Language::Spanish }
    }
}
#[derive(Default, Serialize, Deserialize)]
struct Preferences { #[serde(default)] language: Mode }
fn load(path: &Path) -> Mode {
    let read = || -> Option<Preferences> {
        let file = std::fs::File::open(path).ok()?;
        if file.metadata().ok()?.len() > 4096 { return None; }
        let mut data = Vec::new();
        file.take(4097).read_to_end(&mut data).ok()?;
        if data.len() > 4096 { return None; }
        serde_json::from_slice(&data).ok()
    };
    read().unwrap_or_default().language
}
fn save(path: &Path, language: Mode) -> std::io::Result<()> {
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    crate::private_file::replace(path, &serde_json::to_vec(&Preferences { language })?)
}
fn apply(app: &crate::AppWindow, mode: Mode) {
    let language = mode.resolve(&sys_locale::get_locale().unwrap_or_default());
    // All four language catalogues are compiled into the application.
    slint::select_bundled_translation(language.code()).expect("bundled language");
    i18n::set_language(language);
    app.global::<crate::AppState>().set_language_mode(mode.index());
}
pub(crate) fn install(app: &crate::AppWindow) {
    let path = crate::store::default_path().with_file_name("language.json");
    let current = Rc::new(Cell::new(load(&path)));
    apply(app, current.get());
    let weak = app.as_weak();
    app.global::<crate::AppState>().on_change_language(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let st = app.global::<crate::AppState>();
        let Some(mode) = Mode::from_index(index).filter(|_| !st.get_busy()) else {
            st.set_language_mode(current.get().index()); return;
        };
        if let Err(e) = save(&path, mode) {
            st.set_language_mode(current.get().index());
            crate::toast(&app, &i18n::tr_format!("Sprache konnte nicht gespeichert werden: {e}"), true);
            return;
        }
        current.set(mode);
        apply(&app, mode);
        // Transient output cannot be safely retranslated from rendered strings.
        st.set_toast(crate::Toast::default());
        st.set_unlock_error(Default::default());
        st.set_reader_report(Default::default());
        if st.get_unlocked() {
            crate::refresh_view(&app);
            let detail = st.get_tx_detail();
            if detail.valid {
                let ctx = crate::ctx();
                let vault = ctx.vault.lock().unwrap();
                crate::view::show_detail(&app, &vault, detail.index as usize);
            }
            st.invoke_check_iban(st.get_tf_iban());
            if st.get_bank_results().row_count() > 0 { st.invoke_bank_search(st.get_bank_query()); }
            st.set_bank_picked(Default::default());
            if st.get_reader_names().row_count() == 1 && st.get_reader_names().row_data(0).is_some_and(|s| s.starts_with('(')) {
                st.set_reader_names(ModelRc::new(VecModel::from(vec![i18n::tr("(kein Kartenleser gefunden)").into()])));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_start_uses_supported_system_language_or_english() {
        for (system, expected) in [("de-DE", Language::German), ("en-GB", Language::English), ("fr-CA", Language::French), ("es-ES", Language::Spanish), ("nl-NL", Language::English), ("", Language::English)] {
            assert_eq!(Mode::default().resolve(system), expected);
        }
        assert_eq!(Mode::Fr.resolve("de-DE"), Language::French);
        assert_eq!(Mode::De.resolve("es-ES"), Language::German);
    }
    #[test]
    fn choices_persist_across_restarts_and_invalid_settings_fall_back_to_system() {
        let dir = std::env::temp_dir().join(format!("bankingcraft-language-{:016x}", rand::random::<u64>()));
        let path = dir.join("language.json");
        assert_eq!(load(&path), Mode::System);
        for index in 0..5 {
            let mode = Mode::from_index(index).unwrap();
            save(&path, mode).unwrap();
            assert_eq!(load(&path), mode);
            assert_eq!(mode.index(), index);
        }
        for invalid in ["{}", "invalid", "{\"language\":\"pt\"}"] {
            std::fs::write(&path, invalid).unwrap();
            assert_eq!(load(&path), Mode::System);
        }
        std::fs::write(&path, " ".repeat(4097)).unwrap();
        assert_eq!(load(&path), Mode::System);
        assert!(Mode::from_index(-1).is_none());
        assert!(Mode::from_index(5).is_none());
        save(&path, Mode::En).unwrap();
        assert!(save(&path.join("invalid-child"), Mode::Fr).is_err());
        assert_eq!(load(&path), Mode::En);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod rendering_tests {
    use super::*;
    use crate::{AppState, AppWindow, store::Vault, view};
    use slint::Model;

    #[test]
    fn all_four_bundles_and_models_switch_without_changing_banking_data_or_filters() {
        i_slint_backend_testing::init_no_event_loop();
        let app = AppWindow::new().unwrap();
        let st = app.global::<AppState>();
        let mut vault = Vault::default();
        let ids = vault.merge_accounts(1, vec![fints::Account {
            account_number: "123".into(), product: "Mein Konto".into(),
            iban: "DE89370400440532013000".into(), ..Default::default()
        }]);
        let group = vault.create_group("Privat").unwrap();
        vault.assign_group(ids[0], Some(group)).unwrap();
        let mut tx = crate::business_tests::tx(-1234, "2026-11-01");
        tx.counterparty_name = "Original Empfänger".into();
        tx.purpose = "Original Zweck".into();
        vault.merge_transactions(ids[0], vec![tx]);
        vault.transactions[0].category = Some("Wohnen".into());
        let original = serde_json::to_vec(&vault).unwrap();
        let mut state = view::ViewState::default();
        view::refresh_all(&app, &vault, &mut state);
        st.set_tx_account(1);
        st.set_tx_category("Wohnen".into());
        st.set_tx_period(3);
        st.set_tf_name("Noch nicht gesendet".into());
        st.set_tf_amount("12,34".into());
        for (language, pin, housing) in [
            (Language::German, "PIN-Eingabe", "Wohnen"),
            (Language::English, "PIN entry", "Housing"),
            (Language::French, "Saisie du code PIN", "Logement"),
            (Language::Spanish, "Introducción del PIN", "Vivienda"),
        ] {
            i18n::with_language(language, || {
                slint::select_bundled_translation(language.code()).unwrap();
                assert_eq!(st.get_pin_heading().as_str(), pin, "Slint catalogue {}", language.code());
                view::refresh_all(&app, &vault, &mut state);
                view::show_detail(&app, &vault, 0);
                assert_eq!(st.get_tx_detail().category.as_str(), housing);
                assert_eq!(st.get_tx_detail().name.as_str(), "Original Empfänger");
                assert_eq!(st.get_tx_detail().purpose.as_str(), "Original Zweck");
                assert_eq!(st.get_categories().row_data(1).unwrap().as_str(), "Wohnen");
                assert_eq!(st.get_category_labels().row_data(1).unwrap().as_str(), housing);
                assert_eq!(st.get_tx_category().as_str(), "Wohnen");
                assert_eq!(st.get_tx_account(), 1);
                assert_eq!(st.get_tx_rows().iter().filter(|r| !r.is_header).count(), 1);
                assert_eq!(st.get_group_names().row_data(1).unwrap().as_str(), "Privat");
                assert_eq!(st.get_accounts().row_data(0).unwrap().name.as_str(), "Mein Konto");
                assert_eq!(st.get_tf_name().as_str(), "Noch nicht gesendet");
                assert_eq!(st.get_tf_amount().as_str(), "12,34");
                assert_eq!(view::category_label("Einstellungen"), "Einstellungen", "custom category is not UI text");
                assert_eq!(serde_json::to_vec(&vault).unwrap(), original);
            });
        }
    }
}
