//! Named transfer templates belong to one debtor account in one encrypted vault.
use crate::{store::Vault, ui::ModelRc, AppState, AppWindow};
use banking::SepaTransfer;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Template {
    pub id: u32,
    pub account_id: u32,
    pub name: String,
    pub transfer: SepaTransfer,
}
impl Vault {
    pub fn save_template(
        &mut self,
        account_id: u32,
        name: &str,
        mut transfer: SepaTransfer,
    ) -> Result<u32, String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 60 || name.chars().any(char::is_control) {
            return Err(
                i18n::tr("Bitte einen Vorlagennamen mit 1 bis 60 Zeichen eingeben.").into(),
            );
        }
        if self
            .templates
            .iter()
            .any(|t| t.account_id == account_id && t.name.to_lowercase() == name.to_lowercase())
        {
            return Err(i18n::tr(
                "Für dieses Konto gibt es bereits eine Vorlage mit diesem Namen.",
            )
            .into());
        }
        transfer.execution_date = None;
        transfer.end_to_end_id.clear();
        let mut transfer = self.prepare_transfer(account_id, transfer, false)?;
        transfer.debtor_name.clear();
        transfer.debtor_iban.clear();
        transfer.debtor_bic.clear();
        let id = self.new_id();
        self.templates.push(Template {
            id,
            account_id,
            name: name.into(),
            transfer,
        });
        Ok(id)
    }
    pub fn delete_template(&mut self, account_id: u32, id: u32) -> Result<(), String> {
        let i = self
            .templates
            .iter()
            .position(|t| t.id == id && t.account_id == account_id)
            .ok_or(i18n::tr("Vorlage nicht gefunden."))?;
        self.templates.remove(i);
        Ok(())
    }
}
fn account(app: &AppWindow) -> Option<u32> {
    crate::VIEW.with(|v| {
        v.borrow()
            .sepa_accounts
            .get(app.global::<AppState>().get_tf_account() as usize)
            .copied()
    })
}
pub fn refresh(app: &AppWindow, v: &Vault, vs: &crate::view::ViewState) {
    let st = app.global::<AppState>();
    let account = vs.sepa_accounts.get(st.get_tf_account() as usize).copied();
    let mut rows: Vec<_> = v
        .templates
        .iter()
        .filter(|t| Some(t.account_id) == account)
        .cloned()
        .collect();
    rows.sort_by_key(|t| t.name.to_lowercase());
    // IDs, rather than row indices, keep the selection stable after sorting/deletion.
    if !rows.iter().any(|t| t.id == st.get_tf_template() as u32) {
        st.set_tf_template(0);
    }
    st.set_templates(ModelRc::new(rows));
}
pub fn load(app: &AppWindow, id: u32) {
    let st = app.global::<AppState>();
    let c = crate::ctx();
    if !st.get_unlocked() || st.get_busy() || c.running.load(std::sync::atomic::Ordering::Acquire) {
        return;
    }
    let account = account(app);
    let template = c
        .vault
        .lock()
        .unwrap()
        .templates
        .iter()
        .find(|t| t.id == id && Some(t.account_id) == account)
        .cloned();
    let Some(template) = template else {
        return;
    };
    crate::clear_transfer_form(app);
    st.set_tf_template(id as i32);
    let t = template.transfer;
    st.set_tf_name(t.creditor_name);
    st.set_tf_iban(t.creditor_iban);
    st.set_tf_bic(t.creditor_bic);
    st.set_tf_amount(t.amount.to_string().replace('.', ","));
    st.set_tf_purpose(t.purpose);
    st.set_tf_mode(if t.instant { 1 } else { 0 });
    st.set_tf_date(String::new());
    st.invoke_check_iban(st.get_tf_iban());
    crate::history::record(
        app,
        "Überweisungsvorlage geladen",
        crate::history::Status::Success,
    );
}
pub fn install(app: &AppWindow) {
    let st = app.global::<AppState>();
    let weak = app.as_weak();
    st.on_load_template(move |id| load(&weak.unwrap(), id as u32));
    let weak = app.as_weak();
    st.on_save_template(move || {
        let app = weak.unwrap();
        let st = app.global::<AppState>();
        if !st.get_unlocked() || st.get_busy() || !st.get_show_template_save() {
            return;
        }
        let Some(account) = account(&app) else {
            return;
        };
        // Templates deliberately have no fixed execution date.
        let (transfer, _) = match crate::transfer_template_from_form(&app) {
            Ok(t) => t,
            Err(e) => {
                st.set_template_error(e);
                return;
            }
        };
        let c = crate::ctx();
        if c.running.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        match c.edit_vault_logged("Überweisungsvorlage erstellt", |v| {
            v.save_template(account, &st.get_template_name(), transfer)
        }) {
            Ok(id) => {
                st.set_show_template_save(false);
                st.set_tf_template(id as i32);
                crate::refresh_view(&app);
            }
            Err(e) => st.set_template_error(e),
        }
    });
    let weak = app.as_weak();
    st.on_delete_template(move || {
        let app = weak.unwrap();
        let st = app.global::<AppState>();
        let (Some(account), id) = (account(&app), st.get_tf_template()) else {
            return;
        };
        if id <= 0 || st.get_busy() {
            return;
        }
        let weak = app.as_weak();
        crate::confirm(
            &app,
            i18n::tr("Vorlage löschen?"),
            i18n::tr("Die Vorlage wird nur aus diesem Konto entfernt."),
            true,
            move || {
                crate::edit_vault(&weak.unwrap(), "Überweisungsvorlage gelöscht", |v| {
                    v.delete_template(account, id as u32)
                });
            },
        );
    });
}
#[cfg(test)]
mod tests;
