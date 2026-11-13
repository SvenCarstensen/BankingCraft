use crate::{AppState, AppWindow, PaymentOrderRow, VIEW, ctx, toast, view};
use crate::{payments::OrderStatus, store::Vault};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

fn account_id(app: &AppWindow) -> Option<u32> {
    VIEW.with(|vs| vs.borrow().sepa_accounts.get(app.global::<AppState>().get_tf_account() as usize).copied())
}

fn recipient_id(app: &AppWindow) -> Option<u32> {
    let index = app.global::<AppState>().get_tf_recipient();
    VIEW.with(|vs| index.checked_sub(1).and_then(|i| vs.borrow().recipient_ids.get(i as usize).copied()))
}

pub fn refresh(app: &AppWindow, v: &Vault, vs: &mut view::ViewState) {
    let st = app.global::<AppState>();
    let selected = st.get_tf_recipient().checked_sub(1).and_then(|i| vs.recipient_ids.get(i as usize)).copied();
    let account = vs.sepa_accounts.get(st.get_tf_account() as usize).copied();
    let mut recipients: Vec<_> = v.recipients.iter().filter(|r| Some(r.account_id) == account).collect();
    recipients.sort_by_key(|r| r.name.to_lowercase());
    vs.recipient_ids = recipients.iter().map(|r| r.id).collect();
    let mut names = vec![SharedString::from(i18n::tr("Empfänger auswählen …"))];
    names.extend(recipients.iter().map(|r| SharedString::from(format!("{} · {}", r.name, r.iban))));
    st.set_recipient_names(ModelRc::new(VecModel::from(names)));
    st.set_tf_recipient(selected.and_then(|id| vs.recipient_ids.iter().position(|r| *r == id)).map(|i| i as i32 + 1).unwrap_or(0));
    st.set_queued_order_count(v.payment_orders.iter().filter(|o| o.status == OrderStatus::Queued).count() as i32);
    let rows: Vec<_> = v.payment_orders.iter().rev().map(|o| {
        let needs_review = matches!(o.status, OrderStatus::Submitting | OrderStatus::NeedsReview);
        let status = match o.status {
            OrderStatus::Queued => i18n::tr("Gespeichert"),
            OrderStatus::Sent => i18n::tr("Gesendet"),
            OrderStatus::Submitting | OrderStatus::NeedsReview => i18n::tr("Bitte prüfen"),
        };
        let detail = if needs_review {
            format!("{}\n{}", i18n::tr("Keine automatische Wiederholung. Bitte den Status bei der Bank prüfen, bevor Sie einen neuen Auftrag anlegen."), o.message)
        } else { o.message.clone() };
        PaymentOrderRow {
            id: o.id as i32, name: o.transfer.creditor_name.clone().into(), iban: o.transfer.creditor_iban.clone().into(),
            amount: view::fmt_eur(o.transfer.amount).into(), purpose: o.transfer.purpose.clone().into(),
            account: view::account_label(v, o.account_id).into(),
            execution: o.transfer.execution_date.map(|d| i18n::tr_format!("Ausführung am {}", view::fmt_date(d)))
                .unwrap_or_else(|| if o.transfer.instant { i18n::tr("Echtzeit") } else { i18n::tr("Standard") }.into()).into(),
            status: status.into(), detail: detail.into(), needs_review, queued: o.status == OrderStatus::Queued,
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
        if crate::edit_vault(&app, |v| v.queue_transfer(account, transfer, scheduled, app.global::<AppState>().get_tf_save_recipient()).map(|_| ())) {
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
            st.set_tf_name(r.name.into()); st.set_tf_iban(r.iban.into()); st.set_tf_bic(r.bic.into());
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
            crate::edit_vault(&weak.unwrap(), |v| v.delete_recipient(account, id));
        });
    });
    let weak = app.as_weak();
    st.on_delete_payment_order(move |id| {
        let app = weak.unwrap();
        let queued = ctx().vault.lock().unwrap().payment_orders.iter().any(|o| o.id == id as u32 && o.status == OrderStatus::Queued);
        let text = if queued { i18n::tr("Der gespeicherte Auftrag wird entfernt und nicht gesendet.") }
            else { i18n::tr("Nur der lokale Eintrag wird entfernt. Ein bereits an die Bank übermittelter Auftrag wird dadurch nicht storniert.") };
        let weak = app.as_weak();
        crate::confirm(&app, i18n::tr("Auftrag entfernen?"), text, true, move || {
            crate::edit_vault(&weak.unwrap(), |v| v.delete_payment_order(id as u32));
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bridge::Bridge, jobs::Ctx, store::VaultFile};
    use slint::{Model, platform::{Key, WindowEvent}, LogicalSize};
    use std::sync::{Arc, Mutex, atomic::AtomicBool};

    #[test]
    fn saving_with_enter_is_default_and_address_book_requires_checkbox_for_each_account() {
        i_slint_backend_testing::init_no_event_loop();
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
        app.window().set_size(LogicalSize::new(1360., 1000.));
        crate::refresh_view(&app);
        st.on_send_transfer(|| panic!("Enter must never send to the bank"));
        assert!(!st.get_tf_save_recipient());
        st.set_tf_name("First recipient".into());
        st.set_tf_iban("DE12500105170648489890".into());
        st.set_tf_amount("12,34".into());
        app.show().unwrap();
        let amount = i_slint_backend_testing::ElementHandle::find_by_element_id(&app, "TransferPage::transfer-amount").next().unwrap();
        let input = amount.query_descendants().match_inherits("LineEdit").find_first().unwrap();
        input.mock_single_click(slint::platform::PointerEventButton::Left);
        app.window().dispatch_event(WindowEvent::KeyPressed { text: Key::Return.into() });
        app.window().dispatch_event(WindowEvent::KeyReleased { text: Key::Return.into() });
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
        app.hide().unwrap();
        crate::CTX.with(|context| *context.borrow_mut() = None);
        VIEW.with(|vs| *vs.borrow_mut() = Default::default());
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(test)]
mod visual_review {
    use super::*;
    use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
    use std::rc::Rc;

    /// Optional offline render: cargo test -p bankingcraft render_payment_pages -- --ignored
    #[test]
    #[ignore = "writes synthetic UI screenshots for manual visual review"]
    fn render_payment_pages() {
        struct Platform(Rc<MinimalSoftwareWindow>);
        impl slint::platform::Platform for Platform {
            fn create_window_adapter(&self) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> { Ok(self.0.clone()) }
            fn duration_since_start(&self) -> std::time::Duration { std::time::Duration::ZERO }
        }
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        slint::platform::set_platform(Box::new(Platform(window.clone()))).unwrap();
        let app = AppWindow::new().unwrap();
        let st = app.global::<AppState>();
        st.set_unlocked(true);
        app.global::<crate::Palette>().set_color_scheme(slint::language::ColorScheme::Dark);
        let mut vault = Vault::default(); crate::demo::add_demo(&mut vault);
        let account = vault.accounts[0].id;
        for status in [OrderStatus::Queued, OrderStatus::Sent, OrderStatus::NeedsReview] {
            vault.queue_transfer(account, crate::business_tests::standing().transfer, false, true).unwrap();
            vault.payment_orders.last_mut().unwrap().status = status;
        }
        let mut vs = view::ViewState::default();
        let dir = std::env::temp_dir().join("bankingcraft-payment-review");
        std::fs::create_dir_all(&dir).unwrap();
        for (code, language) in [("de", i18n::Language::German), ("fr", i18n::Language::French)] {
            slint::select_bundled_translation(code).unwrap();
            i18n::with_language(language, || {
                view::refresh_all(&app, &vault, &mut vs);
                for (page, name) in [(3, "transfer"), (9, "orders")] {
                    st.set_page(page);
                    for width in [1100, 1360] {
                        window.set_size(slint::PhysicalSize::new(width, 860));
                        app.show().unwrap();
                        let mut buffer = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(width, 860);
                        app.window().request_redraw();
                        assert!(window.draw_if_needed(|r| { r.render(buffer.make_mut_slice(), width as usize); }));
                        let path = dir.join(format!("{code}-{name}-{width}.png"));
                        image::save_buffer(&path, buffer.as_bytes(), width, 860, image::ColorType::Rgb8).unwrap();
                    }
                }
            });
        }
        app.hide().unwrap();
    }
}
