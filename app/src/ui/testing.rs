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
fn long_bank_error_details_copy_the_complete_message_and_close_with_the_toast() {
    use egui_kittest::kittest::Queryable;
    i18n::with_language(i18n::Language::German, || {
        let app = AppWindow::new().unwrap();
        let st = app.global::<crate::AppState>();
        st.set_unlocked(true);
        let message = format!("Testbank: Umsatzabruf fehlgeschlagen.\n{}\nHBCI: 9160 - Nicht belegt.\n(AqBanking-Status -1)",
            "Synthetischer Bankhinweis ohne Kontodaten.\n".repeat(30));
        crate::toast(&app, &message, true);
        let mut harness = harness(&app, 1100., 680.);
        let preview = message.lines().take(2).collect::<Vec<_>>().join("\n");
        assert!(harness.get_by_label("Details anzeigen").rect().top() >= harness.get_by_label(&preview).rect().bottom());
        harness.get_by_label("Details anzeigen").click();
        harness.run_steps(4);
        harness.get_by_label("Fehlermeldung kopieren").click();
        let mut copied = false;
        for _ in 0..4 {
            harness.step();
            copied |= harness.output().platform_output.commands.iter().any(|command|
                matches!(command, egui::OutputCommand::CopyText(text) if text == &message));
        }
        assert!(copied, "copy must include the offscreen error code and final status");
        if let Some(path) = std::env::var_os("BANKINGCRAFT_ERROR_PREVIEW") {
            harness.render().unwrap().save(path).unwrap();
        }
        st.set_toast(Default::default()); // dismissal / vault-lock cleanup
        harness.run_steps(3);
        assert!(harness.query_by_label("Fehlermeldung kopieren").is_none());
    });
}

#[test]
fn grouped_account_drag_scrolls_at_the_viewport_edge_and_can_be_cancelled() {
    use egui_kittest::kittest::Queryable;
    i18n::with_language(i18n::Language::German, || {
        let app = AppWindow::new().unwrap();
        let st = app.global::<crate::AppState>();
        st.set_unlocked(true); st.set_page(1);
        let mut vault = crate::store::Vault::default();
        vault.merge_accounts(1, (0..8).map(|i| banking::Account {
            account_number: i.to_string(), product: format!("Row {i}"), ..Default::default()
        }).collect());
        crate::view::refresh_all(&app, &vault, &mut Default::default());
        st.on_move_group_account(|_, _, _| panic!("Cancelled scrolling must not move an account"));
        let mut harness = harness(&app, 1100., 680.);
        let source = harness.get_by_label("Konto verschieben: Row 0").rect();
        harness.hover_at(source.center()); harness.drag_at(source.center()); harness.run_steps(2);
        harness.hover_at(egui::pos2(1000., 675.)); harness.run_steps(100);
        let row = harness.query_by_label("Konto verschieben: Row 0");
        assert!(row.is_none_or(|r| r.rect().top() < source.top() - 100.));
        harness.key_press(egui::Key::Escape); harness.run_steps(2);
        harness.drop_at(egui::pos2(1000., 400.)); harness.run_steps(3);
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
fn overview_cards_share_row_heights_after_content_changes_and_resize() {
    use super::{theme::Theme, Model, ModelRc};
    use egui::{Rect, Shape};

    fn card_bounds(shape: &Shape, theme: &Theme, bounds: &mut Vec<Rect>) {
        match shape {
            Shape::Vec(shapes) => {
                for shape in shapes {
                    card_bounds(shape, theme, bounds);
                }
            }
            Shape::Rect(rect)
                if rect.fill == theme.card_bg
                    && rect.stroke.color == theme.card_border
                    && rect.rect.height() > 200. =>
            {
                bounds.push(rect.rect);
            }
            Shape::Mesh(mesh) => {
                // The large mesh is the total-balance gradient; icons are much smaller.
                let rect = mesh.vertices.iter().fold(Rect::NOTHING, |rect, vertex| {
                    rect.union(Rect::from_min_max(vertex.pos, vertex.pos))
                });
                if rect.width() > 200. && rect.height() > 200. {
                    bounds.push(rect);
                }
            }
            _ => {}
        }
    }

    i18n::with_language(i18n::Language::German, || {
        let app = AppWindow::new().unwrap();
        let st = app.global::<crate::AppState>();
        st.set_unlocked(true);
        st.set_theme_mode(2);
        let mut vault = crate::store::Vault::default();
        crate::demo::add_demo(&mut vault);
        crate::view::refresh_all(&app, &vault, &mut Default::default());
        let recent = st.get_recent();
        let categories = st.get_category_stats();
        let upcoming = st.get_upcoming();
        let mut harness = harness(&app, 1360., 1800.);
        let theme = Theme::new(true);
        for width in [1360., 1100., 1900.] {
            harness.set_size(egui::vec2(width, 1800.));
            let mut dense_height = 0.;
            for case in ["normal", "dense", "empty"] {
                st.set_recent(if case == "empty" {
                    Default::default()
                } else {
                    recent.clone()
                });
                st.set_category_stats(if case == "empty" {
                    Default::default()
                } else {
                    categories.clone()
                });
                st.set_upcoming(match case {
                    "empty" => Default::default(),
                    "dense" => ModelRc::new(
                        (0..12)
                            .map(|i| super::state::StandingRow {
                                name: format!("Demo-Dauerauftrag {i}"),
                                amount: "−10,00 €".into(),
                                ..upcoming.row_data(0).unwrap_or_default()
                            })
                            .collect(),
                    ),
                    _ => upcoming.clone(),
                });
                harness.run_steps(4);
                let mut bounds = Vec::new();
                for shape in &harness.output().shapes {
                    card_bounds(&shape.shape, &theme, &mut bounds);
                }
                bounds.sort_by(|a, b| {
                    a.top()
                        .total_cmp(&b.top())
                        .then(a.left().total_cmp(&b.left()))
                });
                assert_eq!(
                    bounds.len(),
                    6,
                    "all overview cards: {width} {case}: {bounds:?}"
                );
                for (row, pair) in bounds.as_chunks::<2>().0.iter().enumerate() {
                    assert!(
                        (pair[0].top() - pair[1].top()).abs() < 0.1,
                        "row {row}: {pair:?}"
                    );
                    assert!(
                        (pair[0].bottom() - pair[1].bottom()).abs() < 0.1,
                        "row {row}: {width} {case}: {pair:?}"
                    );
                    assert!(pair[0].right() < pair[1].left());
                    assert!(pair[1].right() <= width);
                    if row > 0 {
                        assert!(bounds[(row - 1) * 2].bottom() < pair[0].top());
                    }
                }
                if case == "dense" {
                    dense_height = bounds[4].height();
                }
                if case == "empty" {
                    assert!(
                        bounds[4].height() < dense_height,
                        "row must shrink when filtered"
                    );
                }
                if let Some(dir) = std::env::var_os("BANKINGCRAFT_LAYOUT_REVIEW") {
                    let dir = std::path::PathBuf::from(dir);
                    std::fs::create_dir_all(&dir).unwrap();
                    harness
                        .render()
                        .unwrap()
                        .save(dir.join(format!("overview-{width}-{case}.png")))
                        .unwrap();
                }
            }
        }
    });
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
    balance_test_app_with_amounts(100, -1234)
}

fn balance_test_app_with_amounts(booked: i64, pending_cents: i64) -> AppWindow {
    use crate::business_tests::{date, tx};
    let app = AppWindow::new().unwrap();
    let st = app.global::<crate::AppState>();
    st.set_unlocked(true);
    st.set_theme_mode(2);
    let mut vault = crate::store::Vault::default();
    let id = vault.merge_accounts(1, vec![banking::Account {
        product: "Demo current account".into(), account_number: "100000".into(), ..Default::default()
    }])[0];
    vault.accounts[0].balance = Some(banking::Balance {
        booked: rust_decimal::Decimal::from(booked), available: None,
        currency: "EUR".into(), date: date("2026-11-12"),
    });
    let mut pending = tx(pending_cents, "2026-11-13"); pending.pending = true;
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
    for (booked, pending, case) in [(-100, -1234, "negative"), (-10, 1234, "negative-booked")] {
        let app = balance_test_app_with_amounts(booked, pending);
        for (page, name) in [(0, "overview"), (1, "accounts"), (2, "transactions")] {
            app.global::<crate::AppState>().set_page(page);
            snapshot(&app, 1100, 1100, dir.join(format!("{case}-{name}.png")));
        }
    }
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

#[test]
fn paypal_setup_masks_secrets_and_clears_them_when_cancelled_or_provider_changes() {
    use egui_kittest::kittest::Queryable;
    i18n::with_language(i18n::Language::German, || {
        let app = AppWindow::new().unwrap(); let st = app.global::<crate::AppState>();
        st.set_unlocked(true); st.set_page(6); st.set_af_open(true); st.set_af_kind(3);
        st.set_af_user("synthetic-api-user".into());
        st.set_af_paypal_password("synthetic-api-password".into());
        st.set_af_paypal_signature("synthetic-api-signature".into());
        let mut harness = harness(&app, 1360., 1100.);
        assert_eq!(harness.query_all_by_role(egui::accesskit::Role::PasswordInput).count(), 2);
        harness.get_by_label("Abbrechen").click(); harness.run();
        assert!(!st.get_af_open()); assert!(st.get_af_user().is_empty());
        assert!(st.get_af_paypal_password().is_empty()); assert!(st.get_af_paypal_signature().is_empty());
        st.set_af_open(true); st.set_af_kind(3);
        st.set_af_paypal_password("synthetic-api-password".into());
        st.set_af_paypal_signature("synthetic-api-signature".into());
        harness.run(); harness.get_by_label("PIN/TAN (FinTS)").click(); harness.run();
        assert_eq!(st.get_af_kind(), 0);
        assert!(st.get_af_paypal_password().is_empty()); assert!(st.get_af_paypal_signature().is_empty());
    });
}

#[test]
#[ignore = "explicit offline screenshot export; no vault or network access"]
fn render_paypal_setup() {
    for (language, code) in [(i18n::Language::German, "de"), (i18n::Language::English, "en"), (i18n::Language::French, "fr"), (i18n::Language::Spanish, "es")] {
        i18n::with_language(language, || {
            let app = AppWindow::new().unwrap(); let st = app.global::<crate::AppState>();
            st.set_unlocked(true); st.set_page(6); st.set_af_open(true); st.set_af_kind(3); st.set_theme_mode(2);
            st.set_af_name("PayPal Demo".into()); st.set_af_user("demo_api.example.invalid".into());
            st.set_af_paypal_password("synthetic-password".into()); st.set_af_paypal_signature("synthetic-signature".into()); st.set_af_paypal_sandbox(true);
            for width in [1100, 1360] { snapshot(&app, width, 1100, std::env::temp_dir().join(format!("bankingcraft-paypal-{code}-{width}.png"))); }
        });
    }
}
