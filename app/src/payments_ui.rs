use crate::{AppState, AppWindow, PaymentOrderRow, VIEW, ctx, toast, view};
use crate::{payments::OrderStatus, store::Vault};
use crate::ui::{ModelRc, SharedString, VecModel};

fn account_id(app: &AppWindow) -> Option<u32> {
    VIEW.with(|vs| vs.borrow().sepa_accounts.get(app.global::<AppState>().get_tf_account() as usize).copied())
}

fn recipient_id(app: &AppWindow) -> Option<u32> {
    let index = app.global::<AppState>().get_tf_recipient();
    VIEW.with(|vs| index.checked_sub(1).and_then(|i| vs.borrow().recipient_ids.get(i as usize).copied()))
}

pub fn refresh(app: &AppWindow, v: &Vault, vs: &mut view::ViewState) {
    crate::templates::refresh(app,v,vs);
    let st = app.global::<AppState>();
    let selected = st.get_tf_recipient().checked_sub(1).and_then(|i| vs.recipient_ids.get(i as usize)).copied();
    let account = vs.sepa_accounts.get(st.get_tf_account() as usize).copied();
    st.set_tf_instant_available(account.and_then(|id| v.account(id)).is_some_and(|a|
        v.access(a.access_id).is_some_and(|x| x.kind == crate::store::AccessKind::Demo)
        || ["HKIPZ", "HKIPS"].iter().all(|code| a.account.allowed.iter().any(|c| c == code))));
    st.set_tf_scheduled_available(account.and_then(|id| v.account(id)).is_some_and(|a|
        v.access(a.access_id).is_some_and(|x| x.kind == crate::store::AccessKind::Demo)
        || a.account.allowed.iter().any(|c| c == "HKCSE")));
    let mut recipients: Vec<_> = v.recipients.iter().filter(|r| Some(r.account_id) == account).collect();
    recipients.sort_by_key(|r| r.name.to_lowercase());
    vs.recipient_ids = recipients.iter().map(|r| r.id).collect();
    let mut names = vec![SharedString::from(i18n::tr("Empfänger auswählen …"))];
    names.extend(recipients.iter().map(|r| SharedString::from(format!("{} · {}", r.name, r.iban))));
    st.set_recipient_names(ModelRc::new(VecModel::from(names)));
    st.set_tf_recipient(selected.and_then(|id| vs.recipient_ids.iter().position(|r| *r == id)).map(|i| i as i32 + 1).unwrap_or(0));
    st.set_queued_order_count(v.payment_orders.iter().filter(|o| o.status == OrderStatus::Queued).count() as i32);
    // Keep accepted records in the encrypted vault as durable send outcomes,
    // while the outgoing list contains only pending or unresolved orders.
    let rows: Vec<_> = v.payment_orders.iter().rev().filter(|o| o.status != OrderStatus::Sent).map(|o| {
        let needs_review = matches!(o.status, OrderStatus::Submitting | OrderStatus::NeedsReview);
        let status = match o.instant_payment.as_ref().map(|p| p.outcome) {
            Some(banking::InstantOutcome::Pending) => i18n::tr("In Bearbeitung"),
            Some(banking::InstantOutcome::Rejected) => i18n::tr("Abgelehnt"),
            _ => match o.status {
            OrderStatus::Queued => i18n::tr("Gespeichert"),
            OrderStatus::Sent => i18n::tr("Gesendet"),
            OrderStatus::Submitting | OrderStatus::NeedsReview => i18n::tr("Bitte prüfen"),
        }
        };
        let detail = if needs_review {
            format!("{}\n{}", i18n::tr("Keine automatische Wiederholung. Bitte den Status bei der Bank prüfen, bevor Sie einen neuen Auftrag anlegen."), o.message)
        } else { o.message.clone() };
        PaymentOrderRow {
            id: o.id as i32, name: o.transfer.creditor_name.clone(), iban: o.transfer.creditor_iban.clone(),
            amount: view::fmt_eur(o.transfer.amount), purpose: o.transfer.purpose.clone(),
            account: view::account_label(v, o.account_id),
            execution: o.transfer.execution_date.map(|d| i18n::tr_format!("Ausführung am {}", view::fmt_date(d)))
                .unwrap_or_else(|| if o.transfer.instant { i18n::tr("Echtzeit") } else { i18n::tr("Standard") }.into()),
            status: status.into(), detail, needs_review, can_check: o.instant_status_query().is_some(), queued: o.status == OrderStatus::Queued,
        }
    }).collect();
    st.set_payment_orders(ModelRc::new(VecModel::from(rows)));
}

pub fn install(app: &AppWindow) {
    let st = app.global::<AppState>();
    let weak = app.as_weak();
    st.on_save_transfer(move || {
        let app = weak.unwrap();
        if app.global::<AppState>().get_show_confirm() { return; }
        let Some(account) = account_id(&app) else { toast(&app, i18n::tr("Bitte ein Auftraggeberkonto wählen."), true); return; };
        let (transfer, scheduled) = match crate::transfer_from_form(&app) {
            Ok(t) => t, Err(e) => { toast(&app, &e, true); return; },
        };
        if crate::edit_vault(&app, "Überweisung gespeichert", |v| v.queue_transfer(account, transfer, scheduled, app.global::<AppState>().get_tf_save_recipient()).map(|_| ())) {
            crate::clear_transfer_form(&app);
            toast(&app, i18n::tr("Überweisung gespeichert. Versand bei der nächsten Synchronisierung dieses Kontos."), false);
        }
    });
    let weak = app.as_weak();
    st.on_payment_account_changed(move || {
        let app = weak.unwrap();
        let st = app.global::<AppState>();
        // Do not carry an address-book selection over into another account.
        if st.get_tf_recipient() > 0 {
            st.set_tf_name(Default::default()); st.set_tf_iban(Default::default()); st.set_tf_bic(Default::default());
            st.invoke_check_iban(Default::default());
        }
        st.set_tf_recipient(0);
        st.set_tf_template(0);
        st.set_tf_save_recipient(false);
        crate::refresh_view(&app);
    });
    let weak = app.as_weak();
    st.on_select_recipient(move |index| {
        let app = weak.unwrap();
        let st = app.global::<AppState>();
        st.set_tf_recipient(index);
        let id = recipient_id(&app);
        let account = account_id(&app);
        let c = ctx();
        let recipient = c.vault.lock().unwrap().recipients.iter().find(|r| Some(r.id) == id && Some(r.account_id) == account).cloned();
        if let Some(r) = recipient {
            st.set_tf_name(r.name); st.set_tf_iban(r.iban); st.set_tf_bic(r.bic);
        } else {
            st.set_tf_name(Default::default()); st.set_tf_iban(Default::default()); st.set_tf_bic(Default::default());
        }
        st.invoke_check_iban(st.get_tf_iban());
    });
    let weak = app.as_weak();
    st.on_delete_recipient(move || {
        let app = weak.unwrap();
        let (Some(account), Some(id)) = (account_id(&app), recipient_id(&app)) else { return; };
        let weak = app.as_weak();
        crate::confirm(&app, i18n::tr("Empfänger löschen?"), i18n::tr("Der Empfänger wird nur aus dem Adressbuch dieses Kontos entfernt."), true, move || {
            crate::edit_vault(&weak.unwrap(), "Empfänger gelöscht", |v| v.delete_recipient(account, id));
        });
    });
    let weak = app.as_weak();
    st.on_check_payment_status(move |id| {
        ctx().run("Status der Echtzeitüberweisung", move |ctx| crate::jobs::check_instant(ctx, id as u32));
    });
    st.on_delete_payment_order(move |id| {
        let app = weak.unwrap();
        let queued = ctx().vault.lock().unwrap().payment_orders.iter().any(|o| o.id == id as u32 && o.status == OrderStatus::Queued);
        let text = if queued { i18n::tr("Der gespeicherte Auftrag wird entfernt und nicht gesendet.") }
            else { i18n::tr("Nur der lokale Eintrag wird entfernt. Ein bereits an die Bank übermittelter Auftrag wird dadurch nicht storniert.") };
        let weak = app.as_weak();
        crate::confirm(&app, i18n::tr("Auftrag entfernen?"), text, true, move || {
            crate::edit_vault(&weak.unwrap(), "Auftrag gelöscht", |v| v.delete_payment_order(id as u32));
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bridge::Bridge, jobs::Ctx, store::VaultFile};
    use crate::ui::Model;
    use egui_kittest::kittest::Queryable;
    use std::sync::{Arc, Mutex, atomic::AtomicBool};

    #[test]
    fn transfer_modes_require_explicit_capabilities_and_unknown_orders_can_be_checked() {
        i18n::with_language(i18n::Language::German, || {
            let app=AppWindow::new().unwrap();let st=app.global::<AppState>();
            let mut v=Vault::default();crate::demo::add_demo(&mut v);v.accesses[0].kind=crate::store::AccessKind::PinTan;
            let mut vs=view::ViewState::default();
            for (codes,instant,scheduled) in [(vec![],false,false),(vec!["HKIPZ"],false,false),
                (vec!["HKIPZ","HKIPS"],true,false),(vec!["HKCSE"],false,true)] {
                v.accounts[0].account.allowed=codes.into_iter().map(str::to_owned).collect();
                view::refresh_all(&app,&v,&mut vs);
                assert_eq!(st.get_tf_instant_available(),instant);assert_eq!(st.get_tf_scheduled_available(),scheduled);
            }
            st.set_tf_name("Synthetic Recipient".into());st.set_tf_iban("DE02120300000000202051".into());st.set_tf_amount("1,23".into());
            st.set_tf_mode(1);assert!(crate::transfer_from_form(&app).is_err());assert_eq!(st.get_tf_mode(),1,"never silently change an instant template to standard");
            let account=v.accounts[0].id;let mut t=crate::business_tests::standing().transfer;t.instant=true;
            let id=v.queue_transfer(account,t,false,false).unwrap();let order=v.payment_orders.iter_mut().find(|o|o.id==id).unwrap();
            order.status=OrderStatus::NeedsReview;order.instant_payment=Some(banking::InstantPayment{
                debtor_iban:order.transfer.debtor_iban.clone(),reference:"REF".into(),outcome:banking::InstantOutcome::Pending,next_query_at:chrono::Utc::now()});
            let mut v:Vault=serde_json::from_slice(&serde_json::to_vec(&v).unwrap()).unwrap();
            view::refresh_all(&app,&v,&mut vs);let row=st.get_payment_orders().row_data(0).unwrap();
            assert!(row.can_check);assert_eq!(row.status,"In Bearbeitung");
            v.payment_orders[0].instant_payment.as_mut().unwrap().outcome=banking::InstantOutcome::Executed;
            view::refresh_all(&app,&v,&mut vs);assert!(st.get_payment_orders().row_data(0).unwrap().can_check,"lost final acknowledgement remains queryable");
            assert_eq!(v.payment_orders[0].instant_status_query().unwrap().outcome,banking::InstantOutcome::Unknown);
            v.payment_orders[0].status=OrderStatus::Sent;assert!(v.payment_orders[0].instant_status_query().is_none());
            v.payment_orders[0].status=OrderStatus::NeedsReview;
            v.payment_orders[0].instant_payment.as_mut().unwrap().outcome=banking::InstantOutcome::Rejected;
            view::refresh_all(&app,&v,&mut vs);let row=st.get_payment_orders().row_data(0).unwrap();assert!(!row.can_check);assert_eq!(row.status,"Abgelehnt");
            let mut old=serde_json::to_value(&v).unwrap();old["payment_orders"][0].as_object_mut().unwrap().remove("instant_payment");
            let old:Vault=serde_json::from_value(old).unwrap();assert!(old.payment_orders[0].instant_payment.is_none());
        });
    }

    #[test]
    fn bank_accepted_orders_disappear_and_uncertain_orders_remain_after_reload() {
        i18n::with_language(i18n::Language::German, || {
            let app=AppWindow::new().unwrap();let st=app.global::<AppState>();
            st.set_unlocked(true);st.set_page(9);
            let mut vault=Vault::default();crate::demo::add_demo(&mut vault);
            let account=vault.accounts[0].id;
            for (index,status) in [OrderStatus::Queued,OrderStatus::Submitting,OrderStatus::NeedsReview,OrderStatus::Sent,OrderStatus::Sent].into_iter().enumerate() {
                let mut transfer=crate::business_tests::standing().transfer;
                transfer.creditor_name=format!("Synthetic recipient {index}");
                if index==4 {transfer.execution_date=Some(chrono::Local::now().date_naive()+chrono::Duration::days(3));}
                vault.queue_transfer(account,transfer,index==4,false).unwrap();
                let order=vault.payment_orders.last_mut().unwrap();order.status=status;
                // Text cannot convert an uncertain outcome into acceptance.
                order.message="Die Bank hat den Auftrag angenommen.".into();
            }
            let mut vault:Vault=serde_json::from_slice(&serde_json::to_vec(&vault).unwrap()).unwrap();
            let mut vs=view::ViewState::default();view::refresh_all(&app,&vault,&mut vs);
            assert_eq!(st.get_payment_orders().iter().map(|r|r.id).collect::<Vec<_>>(),vault.payment_orders[..3].iter().rev().map(|o|o.id as i32).collect::<Vec<_>>());
            assert_eq!(st.get_queued_order_count(),1);
            let mut harness=crate::ui::testing::harness(&app,1100.,860.);
            assert!(harness.query_by_label("Synthetic recipient 3").is_none());
            assert!(harness.query_by_label("Synthetic recipient 4").is_none());
            vault.payment_orders[0].status=OrderStatus::Sent;
            view::refresh_all(&app,&vault,&mut vs);harness.run_steps(3);
            assert_eq!(st.get_payment_orders().row_count(),2);assert_eq!(st.get_queued_order_count(),0);
            assert!(harness.query_by_label("Synthetic recipient 0").is_none());
            for order in &mut vault.payment_orders {order.status=OrderStatus::Sent;}
            view::refresh_all(&app,&vault,&mut vs);harness.run_steps(3);
            assert_eq!(st.get_payment_orders().row_count(),0);
            harness.get_by_label("Keine offenen Überweisungsaufträge.");
            assert_eq!(vault.payment_orders.len(),5,"durable acceptance records remain available for duplicate-send protection");
        });
    }

    #[test]
    fn saving_with_enter_is_default_and_address_book_requires_checkbox_for_each_account() {

        let app = AppWindow::new().unwrap();
        let st = app.global::<AppState>();
        let path = std::env::temp_dir().join(format!("bankingcraft-payment-ui-{:016x}", rand::random::<u64>()));
        let mut vault = Vault::default(); crate::demo::add_demo(&mut vault);
        let c = Ctx { vault: Arc::new(Mutex::new(vault)),
            file: Arc::new(Mutex::new(Some(VaultFile::create(path.clone(), "test").unwrap()))),
            bridge: Bridge::new(app.as_weak()), running: Arc::new(AtomicBool::new(false)) };
        crate::CTX.with(|context| *context.borrow_mut() = Some(c.clone()));
        VIEW.with(|vs| *vs.borrow_mut() = Default::default());
        install(&app); st.set_unlocked(true); st.set_page(3);
        crate::refresh_view(&app);
        st.on_send_transfer(|| panic!("Enter must never send to the bank"));
        assert!(!st.get_tf_save_recipient());
        st.set_tf_name("First recipient".into());
        st.set_tf_iban("DE12500105170648489890".into());
        st.set_tf_amount("12,34".into());

        let mut harness=crate::ui::testing::harness(&app,1360.,1000.);
        harness.get(egui_kittest::kittest::By::new().role(egui::accesskit::Role::TextInput).value("12,34")).click(); harness.run_steps(2);
        harness.key_press(egui::Key::Enter); harness.run_steps(3);
        assert_eq!(c.vault.lock().unwrap().payment_orders.len(), 1);
        assert!(c.vault.lock().unwrap().recipients.is_empty());
        assert_eq!(st.get_queued_order_count(), 1);
        assert!(st.get_tf_name().is_empty());
        st.set_tf_name("Saved recipient".into());
        st.set_tf_iban("DE12500105170648489890".into());
        st.set_tf_bic("INGDDEFFXXX".into());
        st.set_tf_amount("45,67".into());
        st.set_tf_save_recipient(true); st.invoke_save_transfer();
        assert!(!st.get_tf_save_recipient(), "consent resets for the next payment");
        assert_eq!(st.get_recipient_names().row_count(), 2);
        st.invoke_select_recipient(1);
        assert_eq!(st.get_tf_name(), "Saved recipient");
        assert_eq!(st.get_tf_iban(), "DE12500105170648489890");
        assert_eq!(st.get_tf_bic(), "INGDDEFFXXX");
        st.set_tf_account(1); st.invoke_payment_account_changed();
        assert_eq!(st.get_recipient_names().row_count(), 1);
        assert!(st.get_tf_name().is_empty());
        st.set_tf_name("Other account recipient".into());
        st.set_tf_iban("DE12500105170648489890".into());
        st.set_tf_amount("5,00".into()); st.set_tf_save_recipient(true); st.invoke_save_transfer();
        assert_eq!(c.vault.lock().unwrap().recipients.len(), 2);
        st.set_tf_account(0); st.invoke_payment_account_changed(); st.invoke_select_recipient(1);
        assert_eq!(st.get_tf_name(), "Saved recipient");
        let (_, reloaded) = VaultFile::open(path.clone(), "test").unwrap();
        assert_eq!(reloaded.payment_orders.len(), 3);
        assert_eq!(reloaded.recipients.len(), 2);

        crate::CTX.with(|context| *context.borrow_mut() = None);
        VIEW.with(|vs| *vs.borrow_mut() = Default::default());
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(test)]
mod visual_review {
    use super::*;

    /// Optional offline render: cargo test -p bankingcraft render_payment_pages -- --ignored
    #[test]
    #[ignore = "writes synthetic UI screenshots for manual visual review"]
    fn render_payment_pages() {
        let app = AppWindow::new().unwrap();
        let st = app.global::<AppState>();
        st.set_unlocked(true);
        st.set_theme_mode(2);
        let mut vault = Vault::default(); crate::demo::add_demo(&mut vault);
        let account = vault.accounts[0].id;
        for status in [OrderStatus::Queued, OrderStatus::Sent, OrderStatus::NeedsReview] {
            vault.queue_transfer(account, crate::business_tests::standing().transfer, false, true).unwrap();
            vault.payment_orders.last_mut().unwrap().status = status;
        }
        let mut vs = view::ViewState::default();
        let dir = std::env::temp_dir().join("bankingcraft-egui-payment-review");
        std::fs::create_dir_all(&dir).unwrap();
        for (code, language) in [("de", i18n::Language::German), ("fr", i18n::Language::French)] {
            i18n::with_language(language, || {
                view::refresh_all(&app, &vault, &mut vs);
                for (page, name) in [(3, "transfer"), (9, "orders"), (2, "transactions"), (4, "standing")] {
                    st.set_page(page);
                    for width in [1100, 1360] {
                        crate::ui::testing::snapshot(&app, width, 860, dir.join(format!("{code}-{name}-{width}.png")));
                    }
                }
            });
        }

    }
}
