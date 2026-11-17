use super::*;
use crate::{bridge::Bridge, jobs::Ctx, store::VaultFile, ui::Model};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

pub(crate) struct Fixture {
    pub app: AppWindow,
    pub ctx: Ctx,
    pub dir: std::path::PathBuf,
    pub path: std::path::PathBuf,
    pub account: u32,
}
impl Fixture {
    pub fn new() -> Self {
        let app = AppWindow::new().unwrap();
        let dir = std::env::temp_dir().join(format!(
            "bankingcraft-history-{:016x}",
            rand::random::<u64>()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("test.vault");
        let mut v = Vault::default();
        crate::demo::add_demo(&mut v);
        opened(&mut v);
        let account = v.accounts[0].id;
        let ctx = Ctx {
            vault: Arc::new(Mutex::new(v)),
            file: Arc::new(Mutex::new(Some(
                VaultFile::create(path.clone(), "synthetic-password").unwrap(),
            ))),
            bridge: Bridge::new(app.as_weak()),
            running: Arc::new(AtomicBool::new(false)),
        };
        ctx.bridge.locked.store(false, Ordering::Release);
        ctx.save().unwrap();
        crate::CTX.with(|c| *c.borrow_mut() = Some(ctx.clone()));
        crate::VIEW.with(|v| *v.borrow_mut() = Default::default());
        app.global::<AppState>().set_unlocked(true);
        crate::refresh_view(&app);
        install(&app);
        crate::templates::install(&app);
        Self {
            app,
            ctx,
            dir,
            path,
            account,
        }
    }
    pub fn reload(&self) -> Vault {
        VaultFile::open(self.path.clone(), "synthetic-password")
            .unwrap()
            .1
    }
    pub fn transfer(&self, status: Status) -> u64 {
        let mut v = self.ctx.vault.lock().unwrap();
        let id = v
            .queue_transfer(
                self.account,
                crate::business_tests::standing().transfer,
                false,
                false,
            )
            .unwrap();
        let o = v.payment_orders.iter().find(|o| o.id == id).unwrap();
        let t = Transfer::from_order(&v, o);
        let id = v.history.add("Überweisung eingereicht", status, Some(t));
        drop(v);
        crate::refresh_view(&self.app);
        id
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        crate::CTX.with(|c| *c.borrow_mut() = None);
        crate::VIEW.with(|v| *v.borrow_mut() = Default::default());
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn legacy_vault_recovery_and_import_are_explicit_and_idempotent() {
    let mut v = Vault::default();
    crate::demo::add_demo(&mut v);
    let a = v.accounts[0].id;
    v.queue_transfer(a, crate::business_tests::standing().transfer, false, false)
        .unwrap();
    v.payment_orders[0].status = crate::payments::OrderStatus::Sent;
    v.accesses[0].config.user_id = "synthetic-login-never-copy".into();
    v.payment_orders[0].message = "synthetic-bank-reply-never-copy".into();
    let mut old = serde_json::to_value(&v).unwrap();
    old.as_object_mut().unwrap().remove("history");
    old.as_object_mut().unwrap().remove("templates");
    let mut v: Vault = serde_json::from_value(old).unwrap();
    assert!(v.history.entries.is_empty());
    assert!(v.templates.is_empty());
    opened(&mut v);
    assert!(v.history.entries[0].legacy);
    assert!(v.history.entries[0].note().contains("nicht bekannt"));
    let pending = v
        .history
        .add("Konten synchronisieren", Status::Running, None);
    opened(&mut v);
    assert_eq!(v.history.entries.iter().filter(|e| e.legacy).count(), 1);
    assert_eq!(
        v.history
            .entries
            .iter()
            .find(|e| e.id == pending)
            .unwrap()
            .status,
        Status::Interrupted
    );
    let serialized = serde_json::to_string(&v.history).unwrap();
    assert!(!serialized.contains("never-copy"));
}
#[test]
fn filters_and_sorting_use_time_type_and_stable_ties() {
    i18n::with_language(i18n::Language::German, || {
        let mut h = History::default();
        for status in [
            Status::Error,
            Status::Success,
            Status::Cancelled,
            Status::Running,
        ] {
            h.add("Konten synchronisieren", status, None);
        }
        for e in &mut h.entries {
            e.time = DateTime::from_timestamp(100, 0).unwrap();
        }
        let rows = |q: &str, f, s| select(h.entries.iter(), q, f, s);
        assert_eq!(
            rows("", 0, 0).iter().map(|e| e.id).collect::<Vec<_>>(),
            vec![4, 3, 2, 1]
        );
        assert_eq!(
            rows("", 0, 1).iter().map(|e| e.id).collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert!(rows("", 0, 2)
            .windows(2)
            .all(|w| w[0].status.label() <= w[1].status.label()));
        assert!(rows("", 0, 3)
            .windows(2)
            .all(|w| w[0].status.label() >= w[1].status.label()));
        assert_eq!(rows("konten", 1, 0)[0].status, Status::Success);
        assert_eq!(rows("", 2, 0).len(), 1);
        assert!(rows("missing", 0, 0).is_empty());
    });
}
#[test]
fn encrypted_events_persist_and_failed_mutations_are_not_reported_as_successful() {
    let f = Fixture::new();
    f.ctx
        .edit_vault_logged("Kontogruppe erstellt", |v| {
            v.create_group("Synthetic private group")
        })
        .unwrap();
    assert_eq!(
        f.reload().history.entries.last().unwrap().status,
        Status::Success
    );
    assert!(!String::from_utf8_lossy(&std::fs::read(&f.path).unwrap()).contains("Kontogruppe"));
    assert!(f
        .ctx
        .edit_vault_logged("Kontogruppe erstellt", |v| v
            .create_group("Synthetic private group"))
        .is_err());
    assert_eq!(
        f.reload().history.entries.last().unwrap().status,
        Status::Error
    );
    let original = f.ctx.vault.lock().unwrap().groups.len();
    std::fs::rename(&f.path, f.dir.join("backup")).unwrap();
    std::fs::create_dir(&f.path).unwrap();
    assert!(f
        .ctx
        .edit_vault_logged("Kontogruppe erstellt", |v| v.create_group("Must roll back"))
        .is_err());
    let v = f.ctx.vault.lock().unwrap();
    assert_eq!(v.groups.len(), original);
    assert_eq!(v.history.entries.last().unwrap().status, Status::Error);
    assert!(v.history.entries.last().unwrap().save_failed);
}
#[test]
fn background_results_record_success_error_and_cancel_without_reply_or_secret_text() {
    let f = Fixture::new();
    let egui = egui::Context::default();
    f.app.attach(&egui);
    for status in [Status::Success, Status::Error, Status::Cancelled] {
        f.ctx.run("Konten synchronisieren", move |c| {
            assert_eq!(
                c.vault
                    .lock()
                    .unwrap()
                    .history
                    .entries
                    .last()
                    .unwrap()
                    .status,
                Status::Running
            );
            if status == Status::Cancelled {
                c.bridge.cancelled.store(true, Ordering::Release);
            }
            if status == Status::Success {
                Ok("synthetic-secret-response".into())
            } else {
                Err("synthetic-secret-response".into())
            }
        });
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while f.ctx.running.load(Ordering::Acquire) && std::time::Instant::now() < until {
            f.app.pump(&egui);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!f.ctx.running.load(Ordering::Acquire));
        let v = f.reload();
        assert_eq!(v.history.entries.last().unwrap().status, status);
        assert!(!serde_json::to_string(&v.history)
            .unwrap()
            .contains("synthetic-secret-response"));
    }
}
#[test]
fn repeat_only_fills_new_draft_and_lock_erases_history_and_template_views() {
    let f = Fixture::new();
    let id = f.transfer(Status::Success);
    let st = f.app.global::<AppState>();
    let before = f.ctx.vault.lock().unwrap().payment_orders.len();
    let expected = f
        .ctx
        .vault
        .lock()
        .unwrap()
        .history
        .entries
        .last()
        .unwrap()
        .transfer
        .clone()
        .unwrap();
    st.set_tf_date("01.01.2099".into());
    st.set_tf_mode(2);
    st.invoke_repeat_history(id);
    assert_eq!(st.get_page(), 3);
    assert_eq!(st.get_tf_name(), expected.name);
    assert_eq!(st.get_tf_purpose(), expected.purpose);
    assert_eq!(
        crate::view::parse_amount(&st.get_tf_amount()),
        Some(expected.amount)
    );
    assert!(st.get_tf_date().is_empty());
    assert_eq!(st.get_tf_mode(), 0);
    let (draft, _) = crate::transfer_from_form(&f.app).unwrap();
    assert!(draft.end_to_end_id.is_empty());
    assert!(draft.execution_date.is_none());
    assert_eq!(f.ctx.vault.lock().unwrap().payment_orders.len(), before);
    assert!(!f.ctx.running.load(Ordering::Acquire));
    // Removed debtor accounts cannot silently fall back to another account.
    f.ctx
        .vault
        .lock()
        .unwrap()
        .accounts
        .retain(|a| a.id != f.account);
    crate::refresh_view(&f.app);
    st.set_tf_name("untouched".into());
    st.invoke_repeat_history(id);
    assert_eq!(st.get_tf_name(), "untouched");
    crate::session::lock(&f.app);
    assert_eq!(st.get_history_entries().row_count(), 0);
    assert_eq!(st.get_templates().row_count(), 0);
    assert_eq!(st.get_history_selected(), None);
}
#[test]
fn history_ui_exposes_sort_filter_details_and_repeat_without_sending() {
    use egui_kittest::kittest::Queryable;
    i18n::with_language(i18n::Language::German, || {
        let f = Fixture::new();
        let id = f.transfer(Status::Success);
        let st = f.app.global::<AppState>();
        st.set_page(11);
        st.set_theme_mode(2);
        let mut h = crate::ui::testing::harness(&f.app, 1100., 860.);
        h.query_all_by_role(egui::accesskit::Role::ComboBox)
            .nth(1)
            .unwrap()
            .click();
        h.run_steps(3);
        h.get_by_label("Meldungstyp: A–Z").click();
        h.run_steps(3);
        assert_eq!(st.get_history_sort(), 2);
        h.query_all_by_role(egui::accesskit::Role::ComboBox)
            .next()
            .unwrap()
            .click();
        h.run_steps(3);
        h.get_by_label("Fehler / Bitte prüfen").click();
        h.run_steps(3);
        assert_eq!(st.get_history_filter(), 2);
        assert!(h.query_by_label("Keine passenden Einträge.").is_some());
        st.set_history_filter(0);
        h.run_steps(3);
        let entry = st
            .get_history_entries()
            .iter()
            .find(|e| e.id == id)
            .unwrap();
        let label = format!(
            "{} · {}\n{}\n{}",
            entry.timestamp(),
            entry.status.label(),
            entry.action,
            entry.detail()
        );
        h.get_by_label(&label).click();
        h.run_steps(3);
        assert_eq!(st.get_history_selected(), Some(id));
        assert!(h.query_by_label("Überweisung wiederholen").is_some());
        if let Some(dir) = std::env::var_os("BANKINGCRAFT_HISTORY_PREVIEW") {
            h.render()
                .unwrap()
                .save(std::path::Path::new(&dir).join("history-detail.png"))
                .unwrap();
        }
        h.get_by_label("Überweisung wiederholen").click();
        h.run_steps(3);
        assert_eq!(st.get_page(), 3);
        assert!(!f.ctx.running.load(Ordering::Acquire));
        st.set_page(11);
        st.set_history_selected(None);
        h.run_steps(3);
        if let Some(dir) = std::env::var_os("BANKINGCRAFT_HISTORY_PREVIEW") {
            h.render()
                .unwrap()
                .save(std::path::Path::new(&dir).join("history.png"))
                .unwrap();
        }
    });
}

#[test]
fn history_controls_fit_small_windows_and_large_histories_render_only_visible_rows() {
    use egui_kittest::kittest::Queryable;
    for language in [
        i18n::Language::German,
        i18n::Language::English,
        i18n::Language::French,
        i18n::Language::Spanish,
    ] {
        i18n::with_language(language, || {
            let app = AppWindow::new().unwrap();
            let st = app.global::<AppState>();
            st.set_unlocked(true);
            st.set_page(11);
            let mut vault = Vault::default();
            for _ in 0..5000 {
                vault
                    .history
                    .add("Konten synchronisieren", Status::Success, None);
            }
            refresh(&app, &vault);
            for theme in [1, 2] {
                st.set_theme_mode(theme);
                let h = crate::ui::testing::harness(&app, 1100., 680.);
                let controls: Vec<_> = h
                    .query_all_by_role(egui::accesskit::Role::ComboBox)
                    .map(|n| n.rect())
                    .collect();
                assert_eq!(controls.len(), 2);
                assert!(controls
                    .iter()
                    .all(|r| r.left() >= 236. && r.right() <= 1100. && r.bottom() < 680.));
                assert!(
                    h.query_all_by_role(egui::accesskit::Role::Button).count() < 40,
                    "virtualized, not 5000 row widgets"
                );
                assert!(
                    h.get_by_label(i18n::tr("Einstellungen")).rect().bottom()
                        < h.get_by_label(i18n::tr("Tresor sperren")).rect().top()
                );
            }
        });
    }
}

#[test]
fn old_sync_names_are_normalized_for_display_search_and_persistent_history() {
    i18n::with_language(i18n::Language::German, || {
        let f = Fixture::new();
        for old in ["Kontenrundruf", "Kontenrundruf …", "Kontenrundruf ..."] {
            let mut v = f.ctx.vault.lock().unwrap();
            let id = v
                .history
                .add("Konten synchronisieren", Status::Success, None);
            let entry = v.history.entries.iter_mut().find(|e| e.id == id).unwrap();
            entry.action = old.into(); // Exact data written by earlier versions.
            let mut expected = entry.clone();
            expected.action = "Konten synchronisieren".into();
            assert_eq!(entry.action_label(), "Konten synchronisieren");
            assert!(
                select(v.history.entries.iter(), "Konten synchronisieren", 0, 0)
                    .iter()
                    .any(|e| e.id == id)
            );
            opened(&mut v);
            assert_eq!(
                v.history.entries.iter().find(|e| e.id == id).unwrap(),
                &expected
            );
            drop(v);
            f.ctx.save().unwrap();
            assert_eq!(
                f.reload()
                    .history
                    .entries
                    .iter()
                    .find(|e| e.id == id)
                    .unwrap(),
                &expected
            );
        }
        let mut v = Vault::default();
        v.history
            .add("Überweisung wird übermittelt …", Status::Running, None);
        assert_eq!(
            v.history.entries[0].action_label(),
            "Überweisung wird übermittelt"
        );
    });
}

#[test]
fn long_history_text_wraps_in_full_and_rows_remain_separate_and_clickable() {
    use egui_kittest::kittest::Queryable;
    i18n::with_language(i18n::Language::German, || {
        let f = Fixture::new();
        let id = f.transfer(Status::Success);
        let entry = {
            let mut v = f.ctx.vault.lock().unwrap();
            let e = v.history.entries.iter_mut().find(|e| e.id == id).unwrap();
            e.action = format!(
                "{}ABSCHLUSS DER VOLLSTÄNDIGEN AKTION",
                "Dies ist eine ausführliche Aktion mit vollständig sichtbarem Beschreibungstext. "
                    .repeat(5)
            );
            e.transfer.as_mut().unwrap().name = format!(
                "{}ENDE DES EMPFÄNGERNAMENS",
                "Ein sehr langer Empfängername mit zusätzlichen Angaben ".repeat(4)
            );
            e.clone()
        };
        crate::refresh_view(&f.app);
        let st = f.app.global::<AppState>();
        st.set_page(11);
        st.set_theme_mode(2);
        let label = format!(
            "{} · {}\n{}\n{}",
            entry.timestamp(),
            entry.status.label(),
            entry.action_label(),
            entry.detail()
        );
        let short = st
            .get_history_entries()
            .iter()
            .find(|e| e.transfer.is_none())
            .unwrap();
        let short_label = format!(
            "{} · {}\n{}\n{}",
            short.timestamp(),
            short.status.label(),
            short.action_label(),
            short.detail()
        );
        let mut h = crate::ui::testing::harness(&f.app, 1100., 860.);
        let full = h.get_by_label(&label).rect();
        let next = h.get_by_label(&short_label).rect();
        assert!(full.height() > 150.);
        assert!(full.bottom() <= next.top());
        if let Some(path) = std::env::var_os("BANKINGCRAFT_HISTORY_FULL_TEXT_PREVIEW") {
            h.render().unwrap().save(path).unwrap();
        }
        h.get_by_label(&label).click();
        h.run_steps(3);
        assert_eq!(st.get_history_selected(), Some(id));
        assert!(h.query_by_label("Überweisung wiederholen").is_some());
    });
}
