use super::*;
use crate::{history::tests::Fixture, ui::Model};
#[test]
fn templates_keep_amount_and_purpose_are_account_scoped_and_survive_encrypted_reload() {
    let f = Fixture::new();
    let other = f.ctx.vault.lock().unwrap().accounts[1].id;
    let mut t = crate::business_tests::standing().transfer;
    t.purpose = "Synthetic template purpose".into();
    t.end_to_end_id = "old-payment-id".into();
    t.execution_date = Some(chrono::Local::now().date_naive());
    let id = f
        .ctx
        .edit_vault_logged("Überweisungsvorlage erstellt", |v| {
            v.save_template(f.account, "Monthly", t.clone())
        })
        .unwrap();
    assert!(f
        .ctx
        .edit_vault(|v| v.save_template(f.account, "monthly", t.clone()))
        .is_err());
    f.ctx
        .edit_vault(|v| v.save_template(other, "Monthly", t.clone()))
        .unwrap();
    assert!(f.ctx.edit_vault(|v| v.delete_template(other, id)).is_err());
    let v = f.reload();
    assert_eq!(v.templates.len(), 2);
    assert_eq!(v.templates[0].transfer.amount, t.amount);
    assert_eq!(v.templates[0].transfer.purpose, t.purpose);
    assert!(v.templates[0].transfer.end_to_end_id.is_empty());
    assert!(v.templates[0].transfer.execution_date.is_none());
    assert!(v.templates[0].transfer.debtor_iban.is_empty());
    assert!(!String::from_utf8_lossy(&std::fs::read(&f.path).unwrap())
        .contains("Synthetic template purpose"));
    f.ctx
        .edit_vault(|v| v.delete_template(f.account, id))
        .unwrap();
    assert_eq!(f.reload().templates[0].account_id, other);
    let access = f.reload().account(other).unwrap().access_id;
    f.ctx
        .edit_vault(|v| {
            v.remove_access(access);
            Ok(())
        })
        .unwrap();
    assert!(f.reload().templates.is_empty());
}
#[test]
fn loading_a_template_is_only_a_draft_and_rejects_another_accounts_template() {
    let f = Fixture::new();
    let st = f.app.global::<AppState>();
    let other = f.ctx.vault.lock().unwrap().accounts[1].id;
    let t = crate::business_tests::standing().transfer;
    let id = f
        .ctx
        .edit_vault(|v| v.save_template(f.account, "Rent", t.clone()))
        .unwrap();
    crate::refresh_view(&f.app);
    assert_eq!(st.get_templates().row_count(), 1);
    st.invoke_load_template(id as i32);
    assert_eq!(st.get_tf_name(), t.creditor_name);
    assert_eq!(st.get_tf_purpose(), t.purpose);
    assert_eq!(
        crate::view::parse_amount(&st.get_tf_amount()),
        Some(t.amount)
    );
    assert!(f.ctx.vault.lock().unwrap().payment_orders.is_empty());
    assert!(!f.ctx.running.load(std::sync::atomic::Ordering::Acquire));
    let index = crate::VIEW.with(|v| {
        v.borrow()
            .sepa_accounts
            .iter()
            .position(|i| *i == other)
            .unwrap()
    });
    st.set_tf_account(index as i32);
    crate::refresh_view(&f.app);
    assert_eq!(st.get_templates().row_count(), 0);
    assert_eq!(st.get_tf_template(), 0);
    st.set_tf_name("keep draft".into());
    st.invoke_load_template(id as i32);
    assert_eq!(st.get_tf_name(), "keep draft");
    let index = crate::VIEW.with(|v| {
        v.borrow()
            .sepa_accounts
            .iter()
            .position(|i| *i == f.account)
            .unwrap()
    });
    st.set_tf_account(index as i32);
    crate::refresh_view(&f.app);
    st.invoke_load_template(id as i32);
    crate::payments_ui::install(&f.app);
    st.invoke_save_transfer();
    let v = f.reload();
    assert_eq!(v.payment_orders.len(), 1);
    assert!(!v.payment_orders[0].transfer.end_to_end_id.is_empty());
    assert_eq!(
        v.payment_orders[0].status,
        crate::payments::OrderStatus::Queued
    );
}
#[test]
fn template_dialog_saves_complete_form_and_duplicate_names_show_an_error() {
    use egui_kittest::kittest::Queryable;
    i18n::with_language(i18n::Language::German, || {
        let f = Fixture::new();
        let st = f.app.global::<AppState>();
        st.set_page(3);
        st.set_theme_mode(2);
        let t = crate::business_tests::standing().transfer;
        st.set_tf_name(t.creditor_name);
        st.set_tf_iban(t.creditor_iban);
        st.set_tf_amount("123,45".into());
        st.set_tf_purpose("Monthly subscription".into());
        let mut h = crate::ui::testing::harness(&f.app, 1100., 860.);
        h.get_by_label("Als Vorlage speichern").click();
        h.run_steps(3);
        st.set_template_name("Subscription".into());
        h.run_steps(2);
        h.get_by_label("Vorlage speichern").click();
        h.run_steps(3);
        assert!(!st.get_show_template_save());
        assert_eq!(f.reload().templates.len(), 1);
        assert_eq!(
            f.reload().templates[0].transfer.purpose,
            "Monthly subscription"
        );
        assert!(f.reload().payment_orders.is_empty());
        if let Some(dir) = std::env::var_os("BANKINGCRAFT_HISTORY_PREVIEW") {
            h.render()
                .unwrap()
                .save(std::path::Path::new(&dir).join("templates.png"))
                .unwrap();
        }
        h.get_by_label("Als Vorlage speichern").click();
        h.run_steps(3);
        st.set_template_name("Subscription".into());
        st.invoke_save_template();
        assert!(!st.get_template_error().is_empty());
        assert_eq!(f.reload().templates.len(), 1);
    });
}
