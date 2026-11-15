//! Local account names and hit testing for variable-height overview rows.
use crate::{AppState, AppWindow, AccountDropTarget, ctx, view};
use crate::ui::{Model};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

pub fn install(app: &AppWindow) {
    let st = app.global::<AppState>();
    let weak = app.as_weak();
    st.on_move_group_account(move |source, group, target| {
        crate::edit_vault(&weak.unwrap(), |v| v.move_group_account(
            source as u32,
            (group != 0).then_some(group as u32),
            (target.id >= 0).then_some((target.id as u32, target.after)),
        ))
    });
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
        st.set_account_name_context(format!("{bank} · {number}"));
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

    // The UI reports layout coordinates after wrapping and window resizing. Using the
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
    fn grouped_accounts_drag_between_rows_collapsed_empty_and_ungrouped_targets_atomically() {
        use egui_kittest::kittest::Queryable;
        i18n::with_language(i18n::Language::German, || {
            for width in [1100., 1360.] {
                let app = AppWindow::new().unwrap();
                let st = app.global::<AppState>();
                let path = std::env::temp_dir().join(format!("bankingcraft-group-drag-{:016x}", rand::random::<u64>()));
                let mut vault = Vault::default();
                let ids = vault.merge_accounts(1, ["Alpha", "Beta", "Gamma", "Delta"].into_iter().enumerate()
                    .map(|(i, name)| fints::Account { account_number: i.to_string(), product: name.into(), ..Default::default() }).collect());
                let private = vault.create_group("Privat").unwrap();
                let other = vault.create_group("Weitere").unwrap();
                let empty = vault.create_group("Leer").unwrap();
                for id in &ids[..2] { vault.assign_group(*id, Some(private)).unwrap(); }
                vault.assign_group(ids[2], Some(other)).unwrap();
                let c = Ctx { vault: Arc::new(Mutex::new(vault)),
                    file: Arc::new(Mutex::new(Some(VaultFile::create(path.clone(), "test").unwrap()))),
                    bridge: Bridge::new(app.as_weak()), running: Arc::new(AtomicBool::new(false)) };
                crate::CTX.with(|context| *context.borrow_mut() = Some(c.clone()));
                crate::VIEW.with(|vs| *vs.borrow_mut() = Default::default());
                install(&app); st.set_unlocked(true); st.set_page(1);
                st.set_data_path(path.display().to_string());
                st.on_open_account(|_| panic!("Dragging must not open transactions"));
                crate::refresh_view(&app);
                let mut harness = crate::ui::testing::harness(&app, width, 1600.);
                let drag = |h: &mut egui_kittest::Harness<'static, _>, name: &str, destination: egui::Pos2| {
                    let source = h.get_by_label(&format!("Konto verschieben: {name}")).rect().center();
                    h.hover_at(source); h.drag_at(source); h.run_steps(2);
                    h.hover_at(destination); h.run_steps(2); h.drop_at(destination); h.run_steps(3);
                };
                let beta = harness.get_by_label("Konto verschieben: Beta").rect();
                drag(&mut harness, "Alpha", egui::pos2(beta.center().x, beta.bottom() - 1.));
                assert_eq!(c.vault.lock().unwrap().accounts.iter().map(|a| a.id).collect::<Vec<_>>(), vec![ids[1], ids[0], ids[2], ids[3]]);

                harness.get_by_label("Konten einklappen: Weitere").click(); harness.run_steps(3);
                assert!(harness.query_by_label("Konto verschieben: Gamma").is_none());
                let collapsed = harness.get_by_label("Konten ausklappen: Weitere").rect().center();
                drag(&mut harness, "Alpha", collapsed);
                assert_eq!(c.vault.lock().unwrap().groups[1].account_ids, vec![ids[2], ids[0]]);
                assert!(harness.query_by_label("Konto verschieben: Alpha").is_none());
                harness.get_by_label("Konten ausklappen: Weitere").click(); harness.run_steps(3);
                let target = harness.get_by_label("Konten einklappen: Leer").rect().center();
                drag(&mut harness, "Alpha", target);
                assert_eq!(c.vault.lock().unwrap().groups[2].account_ids, vec![ids[0]]);
                let target = harness.get_by_label("Konto verschieben: Delta").rect().center_top();
                drag(&mut harness, "Alpha", target + egui::vec2(0., 2.));
                assert!(c.vault.lock().unwrap().groups.iter().all(|g| !g.account_ids.contains(&ids[0])));
                assert_eq!(VaultFile::open(path.clone(), "test").unwrap().1.accounts.iter().map(|a| a.id).collect::<Vec<_>>(), vec![ids[1], ids[2], ids[0], ids[3]]);

                let before = serde_json::to_vec(&*c.vault.lock().unwrap()).unwrap();
                let source = harness.get_by_label("Konto verschieben: Alpha").rect().center();
                harness.hover_at(source); harness.drag_at(source); harness.run_steps(2);
                harness.key_press(egui::Key::Escape); harness.run_steps(2);
                let target = harness.get_by_label("Konten einklappen: Privat").rect().center();
                harness.drop_at(target); harness.run_steps(3);
                assert_eq!(serde_json::to_vec(&*c.vault.lock().unwrap()).unwrap(), before);
                drag(&mut harness, "Alpha", egui::pos2(10., 10.));
                assert_eq!(serde_json::to_vec(&*c.vault.lock().unwrap()).unwrap(), before);
                let file = c.file.lock().unwrap().take();
                drag(&mut harness, "Alpha", target);
                assert_eq!(serde_json::to_vec(&*c.vault.lock().unwrap()).unwrap(), before);
                assert!(st.get_toast().error);
                *c.file.lock().unwrap() = file;
                assert_eq!(c.vault.lock().unwrap().groups[2].id, empty);

                // Refresh, navigation and renamed groups retain disclosure state by ID.
                harness.get_by_label("Konten einklappen: Weitere").click(); harness.run_steps(3);
                crate::edit_vault(&app, |v| v.rename_group(other, "Umbenannt"));
                st.set_page(0); harness.run_steps(3); st.set_page(1); harness.run_steps(3);
                assert!(harness.query_by_label("Konto verschieben: Gamma").is_none());
                assert!(harness.query_by_label("Konten ausklappen: Umbenannt").is_some());
                st.set_data_path("another-test-vault".into()); harness.run_steps(3);
                assert!(harness.query_by_label("Konto verschieben: Gamma").is_some());
                st.set_data_path(path.display().to_string()); harness.run_steps(3);
                assert!(harness.query_by_label("Konto verschieben: Gamma").is_none());
                harness.query_all_by_label("Anzeigename ändern").next().unwrap().click(); harness.run_steps(3);
                assert!(st.get_show_account_name(), "Dragging must not intercept the row actions");
                std::fs::remove_file(path).unwrap();
                crate::CTX.with(|context| *context.borrow_mut() = None);
            }
        });
    }

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
        use egui_kittest::kittest::Queryable;
        for width in [1360.,1100.] {
            let mut harness=crate::ui::testing::harness(&app,width,1000.);
            let source=harness.get_by_label(&st.get_accounts().row_data(0).unwrap().name).rect();
            let target=harness.get_by_label(&st.get_accounts().row_data(1).unwrap().name).rect();
            assert!(source.height()>target.height());
            harness.hover_at(source.center()); harness.drag_at(source.center()); harness.run_steps(2);
            let destination=egui::pos2(target.center().x,target.bottom()-5.);
            harness.hover_at(destination); harness.run_steps(2); harness.drop_at(destination); harness.run_steps(3);
            assert_eq!(*moved.borrow(),Some((ids[0],ids[1],true)));*moved.borrow_mut()=None;
            let source=harness.get_by_label(&st.get_accounts().row_data(1).unwrap().name).rect();
            let target=harness.get_by_label(&st.get_accounts().row_data(0).unwrap().name).rect();
            harness.hover_at(source.center());harness.drag_at(source.center());harness.run_steps(2);
            let destination=egui::pos2(target.center().x,target.top()+5.);
            harness.hover_at(destination);harness.run_steps(2);harness.drop_at(destination);harness.run_steps(3);
            assert_eq!(*moved.borrow(),Some((ids[0],ids[1],false)));*moved.borrow_mut()=None;
        }

    }

    #[test]
    fn dragging_uses_wrapped_row_bounds_and_ignores_stale_accounts() {

        let app = AppWindow::new().unwrap();
        install(&app);
        let st = app.global::<AppState>();
        st.set_accounts(crate::ui::ModelRc::new(crate::ui::VecModel::from(vec![
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

    #[test]
    #[ignore = "writes synthetic UI screenshots for manual visual review"]
    fn render_account_names() {
        let app = AppWindow::new().unwrap();
        let st = app.global::<AppState>();
        install(&app);
        st.set_unlocked(true);
        st.set_theme_mode(2);
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
        let dir = std::env::temp_dir().join("bankingcraft-egui-account-name-review");
        std::fs::create_dir_all(&dir).unwrap();
        for (code, language) in [("de", i18n::Language::German), ("fr", i18n::Language::French)] {
            i18n::with_language(language, || {
                for (page, name) in [(0, "overview-tall"), (0, "overview"), (1, "accounts"), (1, "accounts-collapsed"), (1, "rename"), (6, "access-ddv"), (6, "access-rdh"), (-1, "vault-open"), (-1, "vault-new"), (8, "vault-settings"), (3, "vop-match"), (3, "vop-close"), (3, "vop-unavailable")] {
                    view::refresh_all(&app, if name == "overview-tall" { &tall_vault } else { &vault }, &mut vs);
                    st.set_unlocked(page >= 0);
                    st.set_show_vault_manager(false);
                    st.set_first_run(name == "vault-new");
                    st.set_offer_demo_on_create(name == "vault-new");
                    st.set_create_with_demo(name == "vault-new");
                    st.set_active_vault_name("Privat".into());
                    st.set_selected_vault_name(if name == "vault-new" { "Geschäftlich" } else { "Privat" }.into());
                    st.set_selected_vault_path(format!("/Users/demo/Documents/BankingCraft/{}.kontor", st.get_selected_vault_name()));
                    st.set_data_path("/Users/demo/Documents/BankingCraft/Privat.kontor".into());
                    st.set_selected_vault(if name == "vault-new" { 0 } else { 1 });
                    st.set_recent_vaults(crate::ui::ModelRc::new(crate::ui::VecModel::from(vec![i18n::tr("Tresor auswählen …").into(), "Privat · /Users/demo/Documents/BankingCraft".into(), "Verein · /Users/demo/Documents/BankingCraft".into()])));
                    st.set_page(page);
                    st.set_af_open(page == 6);
                    st.set_af_kind(if name == "access-ddv" { 1 } else { 2 });
                    if page == 6 {
                        st.set_accesses(Default::default());
                        st.set_reader_names(crate::ui::ModelRc::new(crate::ui::VecModel::from(vec![crate::ui::SharedString::from(i18n::tr("(kein Kartenleser gefunden)"))])));
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
                    st.set_account_bank_name(vault.accounts[0].account.product.clone());
                    st.set_account_name_context("Demo-Bank · DE00 1234 5678 0000 0000 00".into());
                    st.set_account_name_draft("Meine Reisekarte".into());
                    for width in [1100, 1360] {
                        let height = if name == "overview" { 1360 } else if page < 0 || name.starts_with("vop-") { 680 } else { 860 };
                        let path = dir.join(format!("{code}-{name}-{width}.png"));
                        if name == "accounts-collapsed" {
                            use egui_kittest::kittest::Queryable;
                            let mut harness = crate::ui::testing::harness(&app, width as f32, height as f32);
                            harness.get_by_label(&i18n::tr_format!("Konten einklappen: {group}", group = "Privat")).click();
                            harness.run_steps(3);
                            harness.render().unwrap().save(path).unwrap();
                        } else {
                            crate::ui::testing::snapshot(&app, width, height, path);
                        }
                    }
                }
            });
        }

    }
}
