//! Headless interaction and rendering tests. Never opens a real vault or bank connection.
use super::{render::Desktop, AppWindow};
pub fn harness(
    app: &AppWindow,
    width: f32,
    height: f32,
) -> egui_kittest::Harness<'static, Desktop> {
    let app = app.clone();
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(width, height))
        .build_eframe(move |cc| Desktop::new(app, &cc.egui_ctx));
    harness.run_steps(4);
    harness
}
pub fn snapshot(app: &AppWindow, width: u32, height: u32, path: impl AsRef<std::path::Path>) {
    let mut harness = harness(app, width as f32, height as f32);
    harness
        .render()
        .expect("offscreen renderer")
        .save(path)
        .unwrap();
}

#[test]
fn bank_setup_statement_preference_survives_ui_state_merge() {
    use egui_kittest::kittest::Queryable;
    i18n::with_language(i18n::Language::German, || {
        let app = AppWindow::new().unwrap(); let st = app.global::<crate::AppState>();
        st.set_unlocked(true); st.set_page(6); st.set_af_open(true);
        let mut harness = harness(&app, 1360., 1100.);
        harness.get_by_role(egui::accesskit::Role::ComboBox).click(); harness.run();
        harness.get_by_label("MT940 bevorzugen (ältere Zugänge)").click(); harness.run();
        assert_eq!(st.get_af_statement_format(), 2);
    });
}

#[test]
fn standing_schedule_controls_stay_aligned_at_both_desktop_widths_in_all_languages() {
    use egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;
    for language in [
        i18n::Language::German,
        i18n::Language::English,
        i18n::Language::French,
        i18n::Language::Spanish,
    ] {
        i18n::with_language(language, || {
            let app = AppWindow::new().unwrap();
            let st = app.global::<crate::AppState>();
            st.set_unlocked(true);
            st.set_page(4);
            for width in [1100., 1360.] {
                let harness = harness(&app, width, 860.);
                let edits: Vec<_> = harness
                    .query_all_by_role(Role::TextInput)
                    .map(|n| n.rect())
                    .collect();
                assert_eq!(edits.len(), 7);
                let combos: Vec<_> = harness
                    .query_all_by_role(Role::ComboBox)
                    .map(|n| n.rect())
                    .collect();
                assert_eq!(combos.len(), 2);
                let controls = [combos[1], edits[4], edits[5], edits[6]];
                for pair in controls.windows(2) {
                    assert!(
                        (pair[0].center().y - pair[1].center().y).abs() <= 1.,
                        "unaligned {language:?} {width}: {pair:?}"
                    );
                    assert!(
                        pair[0].right() + 10. <= pair[1].left(),
                        "overlap {language:?} {width}: {pair:?}"
                    );
                }
                assert!(controls[3].right() < width - 24.);
            }
        });
    }
}

#[test]
fn overview_dropdowns_dispatch_independently_and_filter_the_visible_models() {
    use super::Model;
    use egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;
    use std::{cell::RefCell, rc::Rc};
    let app = AppWindow::new().unwrap();
    let st = app.global::<crate::AppState>();
    st.set_unlocked(true);
    let mut vault = crate::store::Vault::default();
    crate::demo::add_demo(&mut vault);
    let group = vault.create_group("Only savings").unwrap();
    vault
        .assign_group(vault.accounts[1].id, Some(group))
        .unwrap();
    let vault = Rc::new(vault);
    let state = Rc::new(RefCell::new(crate::view::ViewState::default()));
    crate::view::refresh_all(&app, &vault, &mut state.borrow_mut());
    let weak = app.as_weak();
    let v = vault.clone();
    let view = state.clone();
    st.on_overview_group_changed(move |tile, index| {
        crate::view::change_overview_group(&weak.unwrap(), &v, &mut view.borrow_mut(), tile, index)
    });
    let old_months = st.get_months();
    let old_categories = st.get_category_stats();
    let old_upcoming = st.get_upcoming();
    let mut harness = harness(&app, 1360., 1360.);
    let first = harness.query_all_by_role(Role::ComboBox).next().unwrap();
    first.click();
    harness.run_steps(3);
    harness
        .query_all_by_label("Only savings")
        .last()
        .unwrap()
        .click();
    harness.run_steps(3);
    assert_ne!(st.get_recent_group(), 0);
    assert!(st.get_recent().iter().all(|row| !row.negative));
    assert_eq!(st.get_months(), old_months);
    assert_eq!(st.get_category_stats(), old_categories);
    assert_eq!(st.get_upcoming(), old_upcoming);
    assert_eq!(
        (
            st.get_category_group(),
            st.get_months_group(),
            st.get_upcoming_group()
        ),
        (0, 0, 0)
    );
}

#[test]
fn changing_vault_selection_wipes_password_drafts() {
    let app = AppWindow::new().unwrap();
    let st = app.global::<crate::AppState>();
    let mut harness = harness(&app, 1100., 680.);
    harness.state_mut().passwords = ["synthetic secret".into(), "repeated secret".into()];
    st.set_vault_selection_revision(st.get_vault_selection_revision() + 1);
    harness.run_steps(3);
    assert!(harness.state().passwords.iter().all(String::is_empty));
}

#[test]
fn long_account_names_do_not_expand_transaction_filter_controls() {
    use egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;
    for language in [
        i18n::Language::German,
        i18n::Language::English,
        i18n::Language::French,
        i18n::Language::Spanish,
    ] {
        i18n::with_language(language, || {
            let app = AppWindow::new().unwrap();
            let st = app.global::<crate::AppState>();
            st.set_unlocked(true);
            st.set_page(2);
            st.set_account_names(super::ModelRc::new(vec![
                "A very long account name with a long bank name".repeat(3),
            ]));
            for width in [1100., 1360.] {
                let harness = harness(&app, width, 860.);
                let account = harness
                    .query_all_by_role(Role::ComboBox)
                    .next()
                    .unwrap()
                    .rect();
                let period = harness.get_by_label(i18n::tr("30 Tage")).rect();
                let search = harness
                    .query_all_by_role(Role::TextInput)
                    .next()
                    .unwrap()
                    .rect();
                assert!(
                    account.right() + 10. <= period.left(),
                    "{language:?} {width}: {account:?} {period:?}"
                );
                assert!((account.center().y - search.center().y).abs() <= 1.);
                assert!((account.center().y - period.center().y).abs() <= 1.);
            }
        });
    }
}

#[test]
fn tan_dialog_keeps_cancel_and_submission_visible_for_all_challenges() {
    use egui_kittest::kittest::Queryable;
    let app = AppWindow::new().unwrap();
    let st = app.global::<crate::AppState>();
    st.set_unlocked(true);
    st.set_show_tan(true);
    st.set_tan_title("Synthetic approval".into());
    st.set_tan_text("Synthetic bank instruction. ".repeat(120));
    st.set_tan_image(super::Image::from_rgba8(
        super::SharedPixelBuffer::clone_from_slice(&[255; 16], 2, 2),
    ));
    st.set_flicker_width(480.);
    let mut harness = harness(&app, 1100., 680.);
    for kind in 0..4 {
        st.set_tan_kind(kind);
        st.set_tan_usb_possible(kind == 1);
        harness.run_steps(4);
        for label in [
            i18n::tr("Abbrechen"),
            i18n::tr(if kind == 3 {
                "Freigegeben"
            } else {
                "TAN senden"
            }),
        ] {
            let r = harness.get_by_label(label).rect();
            assert!(r.top() >= 20. && r.bottom() <= 660., "kind {kind}: {r:?}");
        }
    }
}

#[test]
fn system_theme_tracks_changes_again_after_a_manual_override() {
    let app = AppWindow::new().unwrap();
    let st = app.global::<crate::AppState>();
    let mut harness = harness(&app, 1100., 680.);
    harness.input_mut().system_theme = Some(egui::Theme::Dark);
    harness.run_steps(3);
    assert_eq!(harness.ctx.theme(), egui::Theme::Dark);
    st.set_theme_mode(1);
    harness.run_steps(3);
    assert_eq!(harness.ctx.theme(), egui::Theme::Light);
    st.set_theme_mode(0);
    harness.run_steps(3);
    assert_eq!(harness.ctx.theme(), egui::Theme::Dark);
    harness.input_mut().system_theme = Some(egui::Theme::Light);
    harness.run_steps(3);
    assert_eq!(harness.ctx.theme(), egui::Theme::Light);
}

fn balance_test_app() -> AppWindow {
    use crate::business_tests::{date, tx};
    let app = AppWindow::new().unwrap();
    let st = app.global::<crate::AppState>();
    st.set_unlocked(true);
    st.set_theme_mode(2);
    let mut vault = crate::store::Vault::default();
    let id = vault.merge_accounts(1, vec![fints::Account {
        product: "Demo current account".into(), account_number: "100000".into(), ..Default::default()
    }])[0];
    vault.accounts[0].balance = Some(fints::Balance {
        booked: rust_decimal::Decimal::from(100), available: None,
        currency: "EUR".into(), date: date("2026-11-12"),
    });
    let mut pending = tx(-1234, "2026-11-13"); pending.pending = true;
    vault.merge_transactions(id, vec![pending, tx(-500, "2026-11-12")]);
    st.set_tx_account(1);
    st.set_tx_period(3);
    crate::view::refresh_all(&app, &vault, &mut crate::view::ViewState::default());
    app
}

#[test]
fn balance_labels_and_amounts_are_separate_and_visible_in_all_languages() {
    use egui_kittest::kittest::Queryable;
    for language in [i18n::Language::German, i18n::Language::English, i18n::Language::French, i18n::Language::Spanish] {
        i18n::with_language(language, || {
            let app = balance_test_app();
            let st = app.global::<crate::AppState>();
            for page in [0, 1, 2] {
                st.set_page(page);
                for width in [1100., 1360.] {
                    let harness = harness(&app, width, 1100.);
                    let current: Vec<_> = harness.query_all_by_label(i18n::tr("Saldo inkl. vorgemerkter Umsätze")).map(|n| n.rect()).collect();
                    let booked: Vec<_> = harness.query_all_by_label(i18n::tr("Gebuchter Saldo")).map(|n| n.rect()).collect();
                    assert!(!current.is_empty() && current.len() == booked.len(), "{language:?} page {page}");
                    let amounts: Vec<_> = harness.query_all_by_label("87,66 €").chain(harness.query_all_by_label("100,00 €")).map(|n| n.rect()).collect();
                    assert_eq!(amounts.len(), current.len() * 2);
                    for rect in current.iter().chain(&booked).chain(&amounts) {
                        assert!(rect.left() >= 0. && rect.right() <= width && rect.top() >= 0. && rect.bottom() <= 1100., "{language:?} page {page} width {width}: {rect:?}");
                    }
                    for amount in &amounts {
                        for label in current.iter().chain(&booked) {
                            let overlap = amount.intersect(*label);
                            assert!(overlap.width() <= 0.5 || overlap.height() <= 0.5, "balance overlaps label: {language:?} page {page}: {amount:?} {label:?}");
                        }
                    }
                }
            }
        });
    }
}

#[test]
#[ignore = "exports synthetic screenshots for visual review"]
fn render_balance_review() {
    let dir = std::env::temp_dir().join("bankingcraft-balance-review");
    std::fs::create_dir_all(&dir).unwrap();
    for (code, language) in [("de", i18n::Language::German), ("en", i18n::Language::English), ("fr", i18n::Language::French)] {
        i18n::with_language(language, || {
            let app = balance_test_app();
            let st = app.global::<crate::AppState>();
            for (page, name) in [(0, "overview"), (1, "accounts"), (2, "transactions")] {
                st.set_page(page);
                for (mode, theme) in [(1, "light"), (2, "dark")] {
                    st.set_theme_mode(mode);
                    snapshot(&app, 1100, 1100, dir.join(format!("{code}-{name}-{theme}.png")));
                }
            }
        });
    }
    println!("{}", dir.display());
}
