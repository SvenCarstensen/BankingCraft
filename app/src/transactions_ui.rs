//! Transaction-page actions use the account selected at the time of the click.
use crate::{AppState, AppWindow, VIEW};


pub(crate) fn install(app: &AppWindow) {
    install_with(app, |account| match account {
        Some(id) => crate::ctx().run(i18n::tr("Konto wird aktualisiert …"), move |c| crate::jobs::sync_account(c, id)),
        None => crate::ctx().run(i18n::tr("Kontenrundruf …"), crate::jobs::sync_all),
    });
}

fn install_with(app: &AppWindow, refresh: impl Fn(Option<u32>) + 'static) {
    let weak = app.as_weak();
    app.global::<AppState>().on_refresh_transactions(move || {
        let app = weak.unwrap();
        let index = app.global::<AppState>().get_tx_account();
        if index == 0 {
            refresh(None); // "All accounts" is an explicit choice.
        } else if let Some(id) = VIEW.with(|vs| {
            usize::try_from(index).ok().and_then(|i| i.checked_sub(1))
                .and_then(|i| vs.borrow().account_filter.get(i).copied())
        }) {
            refresh(Some(id));
        } else {
            crate::toast(&app, i18n::tr("Konto nicht gefunden"), true);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    use crate::ui::{ModelRc, VecModel};
    use egui_kittest::kittest::Queryable;

    #[test]
    fn refresh_uses_the_current_account_and_back_returns_to_the_account_list() {

        let app = AppWindow::new().unwrap();
        let st = app.global::<AppState>();
        st.set_unlocked(true);
        st.set_page(2);
        st.set_account_names(ModelRc::new(VecModel::from(vec!["All accounts".into(), "First".into(), "Second".into()])));
        VIEW.with(|vs| vs.borrow_mut().account_filter = vec![42, 7]);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let captured = calls.clone();
        install_with(&app, move |id| captured.borrow_mut().push(id));
        st.on_refresh_all(|| panic!("the transactions button must use its own scoped callback"));
        let weak = app.as_weak();
        st.on_navigate(move |page| weak.unwrap().global::<AppState>().set_page(page));
        let mut harness=crate::ui::testing::harness(&app,1100.,860.);
        st.set_tx_account(1);
        harness.get_by_label(i18n::tr("Umsätze synchronisieren")).click(); harness.run_steps(3);
        st.set_tx_account(2);
        harness.get_by_label(i18n::tr("Umsätze synchronisieren")).click(); harness.run_steps(3);
        assert_eq!(*calls.borrow(), [Some(42), Some(7)]);
        harness.get_by_label(i18n::tr("Zurück zu den Konten")).click(); harness.run_steps(3);
        assert_eq!(st.get_page(), 1);
        assert_eq!(st.get_tx_account(), 2);
        st.set_page(2);
        st.set_tx_account(0);
        harness.run_steps(3);
        harness.get_by_label(i18n::tr("Umsätze synchronisieren")).click(); harness.run_steps(3);
        assert_eq!(*calls.borrow(), [Some(42), Some(7), None]);
        for index in [-1, 99] {
            st.set_tx_account(index);
            st.invoke_refresh_transactions();
        }
        assert_eq!(calls.borrow().len(), 3, "an invalid selection must not trigger an all-account refresh");

        VIEW.with(|vs| *vs.borrow_mut() = Default::default());
    }
}
