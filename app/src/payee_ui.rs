//! Displays bank verification before the worker may continue to payment authorization.
use crate::{AppState, AppWindow, bridge::{Answer, Bridge}};
use fints::vop::{PayeeStatus, PayeeVerification};
use slint::ComponentHandle;

pub fn show(app: &AppWindow, result: &PayeeVerification, already_executed: bool) {
    let st = app.global::<AppState>();
    st.set_vop_status(match result.status { PayeeStatus::Match => 0, PayeeStatus::CloseMatch => 1, PayeeStatus::NoMatch => 2, PayeeStatus::Unavailable => 3 });
    st.set_vop_name(result.name.clone().into());
    st.set_vop_iban(result.iban.clone().into());
    st.set_vop_suggested_name(result.suggested_name.clone().into());
    st.set_vop_reason(result.reason.clone().into());
    st.set_vop_notice(result.bank_notice.clone().into());
    st.set_vop_executed(already_executed);
    st.set_show_vop(true);
}
pub fn clear(st: &AppState<'_>) {
    st.set_show_vop(false);
    st.set_vop_name(Default::default()); st.set_vop_iban(Default::default());
    st.set_vop_suggested_name(Default::default()); st.set_vop_reason(Default::default()); st.set_vop_notice(Default::default());
}
pub fn install(app: &AppWindow, bridge: &Bridge) {
    let st = app.global::<AppState>();
    let weak = app.as_weak(); let b = bridge.clone();
    st.on_vop_accept(move || { clear(&weak.unwrap().global::<AppState>()); b.answer(Answer::Confirmed); });
    let weak = app.as_weak(); let b = bridge.clone();
    st.on_vop_cancel(move || { clear(&weak.unwrap().global::<AppState>()); b.answer(Answer::Cancel); });
    let weak = app.as_weak();
    st.on_vop_copy_name(move || {
        let app = weak.unwrap(); let st = app.global::<AppState>();
        if let Ok(mut clipboard) = arboard::Clipboard::new() { let _ = clipboard.set_text(st.get_vop_suggested_name().to_string()); }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verification_dialog_keeps_actions_visible_and_clears_sensitive_data_on_answer() {
        i_slint_backend_testing::init_no_event_loop();
        let app = AppWindow::new().unwrap(); let bridge = Bridge::new(app.as_weak()); install(&app, &bridge);
        let st = app.global::<AppState>(); st.set_unlocked(true);
        app.window().set_size(slint::LogicalSize::new(1100., 680.)); app.show().unwrap();
        for status in [PayeeStatus::Match, PayeeStatus::CloseMatch, PayeeStatus::NoMatch, PayeeStatus::Unavailable] {
            let result = PayeeVerification { status, name: "Synthetic Receiver".into(), iban: "DE12500105170648489890".into(), suggested_name: "Synthetic Receiver GmbH".into(), reason: "Synthetic reason".into(), bank_notice: "Long bank notice. ".repeat(1000) };
            show(&app, &result, false);
            let footer = i_slint_backend_testing::ElementHandle::find_by_element_id(&app, "PayeeDialog::footer").next().unwrap();
            assert!(footer.size().height > 0.); assert!(footer.absolute_position().y + footer.size().height <= 680.);
            let (tx, rx) = std::sync::mpsc::channel(); *bridge.pending.lock().unwrap() = Some(tx);
            st.invoke_vop_cancel(); assert!(matches!(rx.try_recv(), Ok(Answer::Cancel))); assert!(!st.get_show_vop()); assert!(st.get_vop_notice().is_empty());
            show(&app, &result, false);
            let (tx, rx) = std::sync::mpsc::channel(); *bridge.pending.lock().unwrap() = Some(tx);
            st.invoke_vop_accept(); assert!(matches!(rx.try_recv(), Ok(Answer::Confirmed))); assert!(st.get_vop_iban().is_empty());
        }
        app.hide().unwrap();
    }
}
