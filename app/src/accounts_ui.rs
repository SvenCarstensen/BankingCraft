//! Local account names and hit testing for variable-height overview rows.
use crate::{AppState, AppWindow, AccountDropTarget, ctx, view};
use slint::{ComponentHandle, Model};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

pub fn install(app: &AppWindow) {
    let st = app.global::<AppState>();
    let weak = app.as_weak();
    st.on_edit_account_name(move |id| {
        let app = weak.unwrap();
        let st = app.global::<AppState>();
        let c = ctx();
        let vault = c.vault.lock().unwrap();
        let Some(account) = vault.account(id as u32) else { return; };
        st.set_editing_account_id(id);
        let name = if account.display_name.is_empty() { view::bank_account_name(&account.account) } else { &account.display_name };
        st.set_account_name_draft(name.into());
        st.set_account_bank_name(view::bank_account_name(&account.account).into());
        let bank = vault.access(account.access_id).map(|a| a.name.as_str()).unwrap_or_default();
        let number = if account.account.iban.is_empty() { &account.account.account_number } else { &account.account.iban };
        st.set_account_name_context(format!("{bank} · {number}").into());
        st.set_show_account_name(true);
    });
    let weak = app.as_weak();
    st.on_save_account_name(move || {
        let app = weak.unwrap();
        let st = app.global::<AppState>();
        if !st.get_show_account_name() { return; }
        if crate::edit_vault(&app, |v| {
            let id = st.get_editing_account_id() as u32;
            let name = st.get_account_name_draft();
            // Saving an unchanged bank name must not create a local override.
            if v.account(id).is_some_and(|a| a.display_name.is_empty() && name.trim() == view::bank_account_name(&a.account)) {
                return Ok(());
            }
            v.rename_account(id, &name)
        }) {
            st.set_show_account_name(false);
            // Refresh an already-open transaction detail as well as the list models.
            let c = ctx();
            let vault = c.vault.lock().unwrap();
            let detail = st.get_tx_detail();
            if detail.valid { view::show_detail(&app, &vault, detail.index as usize); }
        }
    });

    // Slint reports layout coordinates after wrapping and window resizing. Using the
    // actual bounds keeps drag insertion independent of the number of name lines.
    let rows = Rc::new(RefCell::new(HashMap::<i32, (f32, f32)>::new()));
    let layout = rows.clone();
    st.on_account_row_layout(move |id, top, height| { layout.borrow_mut().insert(id, (top, height)); });
    let weak = app.as_weak();
    st.on_account_drop_target(move |y| {
        let app = weak.unwrap();
        let rows = rows.borrow();
        let accounts = app.global::<AppState>().get_accounts();
        let mut previous_bottom = None;
        for account in accounts.iter() {
            let Some(&(top, height)) = rows.get(&account.id) else { continue; };
            let start = previous_bottom.unwrap_or(top);
            if y >= start && y < top + height {
                return AccountDropTarget { id: account.id, after: y > top + height / 2.0 };
            }
            previous_bottom = Some(top + height);
        }
        AccountDropTarget { id: -1, after: false }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bridge::Bridge, jobs::Ctx, store::{Vault, VaultFile}};
    use std::sync::{Arc, Mutex, atomic::AtomicBool};

    #[test]
    fn display_name_survives_bank_refresh_and_legacy_vaults_still_load() {
        let mut vault = Vault::default();
        let mut bank_account = fints::Account { account_number: "123".into(), product: "Original bank name".into(), ..Default::default() };
        let id = vault.merge_accounts(1, vec![bank_account.clone()])[0];
        vault.rename_account(id, "  Meine Rücklagen für die Familie  ").unwrap();
        bank_account.product = "Updated bank name".into();
        assert_eq!(vault.merge_accounts(1, vec![bank_account]), vec![id]);
        let reloaded: Vault = serde_json::from_slice(&serde_json::to_vec(&vault).unwrap()).unwrap();
        assert_eq!(reloaded.account(id).unwrap().display_name, "Meine Rücklagen für die Familie");
        assert_eq!(reloaded.account(id).unwrap().account.product, "Updated bank name");
        assert!(vault.rename_account(id, "Embedded\nnewline").is_err());
        assert!(vault.rename_account(999, "Missing").is_err());
        assert_eq!(vault.account(id).unwrap().display_name, "Meine Rücklagen für die Familie");
        // Long Unicode names are retained in full, not silently shortened.
        let long = "Urlaub – Rücklagen 🏖️ ".repeat(20);
        vault.rename_account(id, &long).unwrap();
        assert_eq!(vault.account(id).unwrap().display_name, long.trim());
        vault.rename_account(id, "   ").unwrap();
        assert_eq!(view::account_label(&vault, id), "Updated bank name · ");
        let mut legacy = serde_json::to_value(&vault).unwrap();
        legacy["accounts"][0].as_object_mut().unwrap().remove("display_name");
        assert!(serde_json::from_value::<Vault>(legacy).unwrap().account(id).unwrap().display_name.is_empty());
    }

    #[test]
    fn rename_dialog_persists_updates_all_views_and_rolls_back_failed_saves() {
        i_slint_backend_testing::init_no_event_loop();
        let app = AppWindow::new().unwrap();
        let st = app.global::<AppState>();
        let path = std::env::temp_dir().join(format!("bankingcraft-account-name-{:016x}", rand::random::<u64>()));
        let mut vault = Vault::default(); crate::demo::add_demo(&mut vault);
        let id = vault.accounts[0].id;
        let original = vault.accounts[0].account.product.clone();
        let c = Ctx { vault: Arc::new(Mutex::new(vault)),
            file: Arc::new(Mutex::new(Some(VaultFile::create(path.clone(), "test").unwrap()))),
            bridge: Bridge::new(app.as_weak()), running: Arc::new(AtomicBool::new(false)) };
        crate::CTX.with(|context| *context.borrow_mut() = Some(c.clone()));
        crate::VIEW.with(|vs| *vs.borrow_mut() = Default::default());
        install(&app); st.set_unlocked(true); st.set_page(1);
        crate::refresh_view(&app);
        st.set_tx_account(1);
        st.invoke_edit_account_name(id as i32);
        assert!(st.get_show_account_name());
        assert_eq!(st.get_account_bank_name(), original);
        assert_eq!(st.get_account_name_draft(), original);
        st.invoke_save_account_name();
        assert!(c.vault.lock().unwrap().account(id).unwrap().display_name.is_empty());
        st.invoke_edit_account_name(id as i32);
        st.set_account_name_draft("Cancelled".into());
        st.set_show_account_name(false);
        st.invoke_edit_account_name(id as i32);
        assert_eq!(st.get_account_name_draft(), original);
        st.set_account_name_draft("Mein Haushaltskonto".into());
        st.invoke_save_account_name();
        assert!(!st.get_show_account_name());
        assert_eq!(st.get_tx_account(), 1);
        assert_eq!(st.get_accounts().row_data(0).unwrap().name, "Mein Haushaltskonto");
        assert!(st.get_account_names().row_data(1).unwrap().starts_with("Mein Haushaltskonto · "));
        assert!(st.get_sepa_accounts().row_data(0).unwrap().starts_with("Mein Haushaltskonto · "));
        let (_, saved) = VaultFile::open(path.clone(), "test").unwrap();
        assert_eq!(saved.account(id).unwrap().display_name, "Mein Haushaltskonto");
        assert_eq!(saved.account(id).unwrap().account.product, original);
        st.invoke_edit_account_name(id as i32);
        assert_eq!(st.get_account_name_draft(), "Mein Haushaltskonto");
        st.set_account_name_draft("Not saved".into());
        let file = c.file.lock().unwrap().take();
        st.invoke_save_account_name();
        assert!(st.get_show_account_name());
        assert_eq!(c.vault.lock().unwrap().account(id).unwrap().display_name, "Mein Haushaltskonto");
        *c.file.lock().unwrap() = file;
        st.set_account_name_draft("".into()); st.invoke_save_account_name();
        assert_eq!(st.get_accounts().row_data(0).unwrap().name, original);
        assert!(VaultFile::open(path.clone(), "test").unwrap().1.account(id).unwrap().display_name.is_empty());
        st.invoke_edit_account_name(id as i32);
        assert_eq!(st.get_account_name_draft(), original);
        std::fs::remove_file(path).unwrap();
        crate::CTX.with(|context| *context.borrow_mut() = None);
    }

    #[test]
    fn overview_drag_targets_variable_height_rows_after_layout_and_resize() {
        i_slint_backend_testing::init_no_event_loop();
        let app = AppWindow::new().unwrap();
        install(&app);
        let st = app.global::<AppState>();
        let mut vault = Vault::default(); crate::demo::add_demo(&mut vault);
        vault.accounts[0].display_name = "Rücklagen für Urlaub, Familie und größere Anschaffungen".repeat(3);
        let ids: Vec<_> = vault.accounts.iter().map(|a| a.id as i32).collect();
        st.set_unlocked(true); st.set_page(0);
        view::refresh_all(&app, &vault, &mut Default::default());
        let moved = Rc::new(RefCell::new(None));
        let result = moved.clone();
        let vault = Rc::new(RefCell::new(vault));
        let weak = app.as_weak();
        st.on_move_account(move |source, target, after| {
            *result.borrow_mut() = Some((source, target, after));
            let mut vault = vault.borrow_mut();
            vault.move_account(source as u32, target as u32, after).unwrap();
            view::refresh_all(&weak.unwrap(), &vault, &mut Default::default());
            true
        });
        st.on_open_account(|_| panic!("Dragging must not open transactions"));
        for width in [1360., 1100.] {
            app.window().set_size(slint::LogicalSize::new(width, 1000.));
            app.show().unwrap();
            let rows: Vec<_> = i_slint_backend_testing::ElementHandle::find_by_element_id(&app, "OverviewPage::account-row").collect();
            assert_eq!(rows.len(), 2);
            assert!(rows[0].size().height > rows[1].size().height);
            i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(20));
            let target = rows[1].absolute_position();
            rows[0].mock_drag(slint::LogicalPosition::new(target.x + rows[1].size().width / 2., target.y + rows[1].size().height - 5.), slint::platform::PointerEventButton::Left);
            assert_eq!(*moved.borrow(), Some((ids[0], ids[1], true)));
            *moved.borrow_mut() = None;
            let rows: Vec<_> = i_slint_backend_testing::ElementHandle::find_by_element_id(&app, "OverviewPage::account-row").collect();
            // The repeater can reuse row items after sorting; bounds must follow IDs.
            let target = rows[0].absolute_position();
            i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(20));
            rows[1].mock_drag(slint::LogicalPosition::new(target.x + rows[0].size().width / 2., target.y + 5.), slint::platform::PointerEventButton::Left);
            assert_eq!(*moved.borrow(), Some((ids[0], ids[1], false)));
            *moved.borrow_mut() = None;
        }
        app.hide().unwrap();
    }

    #[test]
    fn dragging_uses_wrapped_row_bounds_and_ignores_stale_accounts() {
        i_slint_backend_testing::init_no_event_loop();
        let app = AppWindow::new().unwrap();
        install(&app);
        let st = app.global::<AppState>();
        st.set_accounts(slint::ModelRc::new(slint::VecModel::from(vec![
            crate::AccountRow { id: 10, ..Default::default() },
            crate::AccountRow { id: 20, ..Default::default() },
            crate::AccountRow { id: 30, ..Default::default() },
        ])));
        st.invoke_account_row_layout(10, 40., 100.);
        st.invoke_account_row_layout(20, 144., 54.);
        st.invoke_account_row_layout(30, 202., 160.);
        st.invoke_account_row_layout(99, 0., 400.);
        assert_eq!(st.invoke_account_drop_target(39.).id, -1);
        assert_eq!(st.invoke_account_drop_target(362.).id, -1);
        for (y, id, after) in [(45., 10, false), (139., 10, true), (142., 20, false), (175., 20, true), (210., 30, false), (350., 30, true)] {
            let target = st.invoke_account_drop_target(y);
            assert_eq!((target.id, target.after), (id, after));
        }
    }
}

#[cfg(test)]
mod visual_review {
    use super::*;
    use crate::store::Vault;
    use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};

    #[test]
    #[ignore = "writes synthetic UI screenshots for manual visual review"]
    fn render_account_names() {
        struct Platform(Rc<MinimalSoftwareWindow>);
        impl slint::platform::Platform for Platform {
            fn create_window_adapter(&self) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> { Ok(self.0.clone()) }
            fn duration_since_start(&self) -> std::time::Duration { std::time::Duration::ZERO }
        }
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        slint::platform::set_platform(Box::new(Platform(window.clone()))).unwrap();
        let app = AppWindow::new().unwrap();
        let st = app.global::<AppState>();
        install(&app);
        st.set_unlocked(true);
        app.global::<crate::Palette>().set_color_scheme(slint::language::ColorScheme::Dark);
        let mut vault = Vault::default(); crate::demo::add_demo(&mut vault);
        vault.accounts[0].account.product = "Postbank Visa Card (Kreditkarte für private Ausgaben und Reisen)".into();
        vault.accounts[1].display_name = "Meine Rücklagen für Urlaub, Familie und größere Anschaffungen".into();
        let group = vault.create_group("Privat").unwrap();
        vault.assign_group(vault.accounts[0].id, Some(group)).unwrap();
        let mut tall_vault = vault.clone();
        tall_vault.merge_accounts(tall_vault.accounts[0].access_id, (0..8).map(|i| fints::Account {
            account_number: format!("900{i}"), product: format!("Weiteres Demokonto {}", i + 1), ..Default::default()
        }).collect());
        let mut vs = view::ViewState::default();
        let dir = std::env::temp_dir().join("bankingcraft-account-name-review");
        std::fs::create_dir_all(&dir).unwrap();
        for (code, language) in [("de", i18n::Language::German), ("fr", i18n::Language::French)] {
            slint::select_bundled_translation(code).unwrap();
            i18n::with_language(language, || {
                for (page, name) in [(0, "overview-tall"), (0, "overview"), (1, "accounts"), (1, "rename"), (6, "access-ddv"), (6, "access-rdh"), (-1, "vault-open"), (-1, "vault-new"), (8, "vault-settings"), (3, "vop-match"), (3, "vop-close"), (3, "vop-unavailable")] {
                    view::refresh_all(&app, if name == "overview-tall" { &tall_vault } else { &vault }, &mut vs);
                    st.set_unlocked(page >= 0);
                    st.set_show_vault_manager(false);
                    st.set_first_run(name == "vault-new");
                    st.set_active_vault_name("Privat".into());
                    st.set_selected_vault_name(if name == "vault-new" { "Geschäftlich" } else { "Privat" }.into());
                    st.set_selected_vault_path(format!("/Users/demo/Documents/BankingCraft/{}.kontor", st.get_selected_vault_name()).into());
                    st.set_data_path("/Users/demo/Documents/BankingCraft/Privat.kontor".into());
                    st.set_selected_vault(if name == "vault-new" { 0 } else { 1 });
                    st.set_recent_vaults(slint::ModelRc::new(slint::VecModel::from(vec![i18n::tr("Tresor auswählen …").into(), "Privat · /Users/demo/Documents/BankingCraft".into(), "Verein · /Users/demo/Documents/BankingCraft".into()])));
                    st.set_page(page);
                    st.set_af_open(page == 6);
                    st.set_af_kind(if name == "access-ddv" { 1 } else { 2 });
                    if page == 6 {
                        st.set_accesses(Default::default());
                        st.set_reader_names(slint::ModelRc::new(slint::VecModel::from(vec![slint::SharedString::from(i18n::tr("(kein Kartenleser gefunden)"))])));
                        st.set_af_name("Demo-Bank".into());
                        st.set_af_record("1".into());
                        st.set_af_blz("12345678".into());
                        st.set_af_user("demo".into());
                        st.set_af_url("bank.example".into());
                    }
                    crate::payee_ui::clear(&st);
                    if name.starts_with("vop-") {
                        crate::payee_ui::show(&app, &fints::vop::PayeeVerification {
                            status: match name { "vop-match" => fints::vop::PayeeStatus::Match, "vop-close" => fints::vop::PayeeStatus::CloseMatch, _ => fints::vop::PayeeStatus::Unavailable },
                            name: "Beispielhandel für nachhaltige Haushaltswaren".into(), iban: "DE12500105170648489890".into(),
                            suggested_name: if name == "vop-close" { "Beispielhandel für nachhaltige Haushaltswaren und regionale Produkte GmbH".into() } else { String::new() },
                            reason: if name == "vop-unavailable" { i18n::tr("Die Bank hat kein Prüfergebnis geliefert.").into() } else { String::new() },
                            bank_notice: "Dies ist ein fiktiver Bankhinweis für die Layoutprüfung. Bei Abweichungen prüfen Sie bitte den Empfängernamen und die IBAN.\n\nEin weiterer Absatz mit Informationen der Bank.".repeat(4),
                        }, false);
                    }
                    st.set_show_account_name(name == "rename");
                    st.set_account_bank_name(vault.accounts[0].account.product.clone().into());
                    st.set_account_name_context("Demo-Bank · DE00 1234 5678 0000 0000 00".into());
                    st.set_account_name_draft("Meine Reisekarte".into());
                    for width in [1100, 1360] {
                        let height = if page < 0 || name.starts_with("vop-") { 680 } else { 860 };
                        window.set_size(slint::PhysicalSize::new(width, height));
                        app.show().unwrap();
                        let mut buffer = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(width, height);
                        app.window().request_redraw();
                        assert!(window.draw_if_needed(|r| { r.render(buffer.make_mut_slice(), width as usize); }));
                        image::save_buffer(dir.join(format!("{code}-{name}-{width}.png")), buffer.as_bytes(), width, height, image::ColorType::Rgb8).unwrap();
                    }
                }
            });
        }
        app.hide().unwrap();
    }
}
