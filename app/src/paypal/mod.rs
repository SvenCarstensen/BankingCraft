//! PayPal is a separate, read-only provider; it never enters a FinTS dialog.
mod client;
use crate::{jobs::{Ctx, JobResult}, store::{AccessKind, BankAccess, StoredTx, Vault}, ui::{AppWindow, state::AppState}};
use chrono::{Duration, Utc};
use client::{Client, Credentials, Http, Snapshot};
use banking::{Account, Balance};
use rust_decimal::Decimal;
use std::{collections::{BTreeSet, HashMap, HashSet}, sync::atomic::Ordering};
use zeroize::Zeroizing;

pub fn is_paypal(v: &Vault, account_id: u32) -> bool {
    v.account(account_id).and_then(|a| v.access(a.access_id)).is_some_and(|a| matches!(a.kind, AccessKind::PayPal { .. }))
}
pub fn read_only_error() -> String { i18n::tr("Der PayPal-Zugang unterstützt nur Guthaben und Umsätze, keine Zahlungsaufträge.").into() }

pub fn save_access(app: &AppWindow, ctx: &Ctx) -> Result<u32, String> {
    let st = app.global::<AppState>();
    let credentials = Credentials { user: st.get_af_user().trim().into(), password: st.get_af_paypal_password().trim().into(), signature: st.get_af_paypal_signature().trim().into() };
    credentials.validate()?;
    let json = Zeroizing::new(serde_json::to_string(&credentials).map_err(|_| i18n::tr("PayPal-Zugangsdaten konnten nicht gespeichert werden."))?);
    // Release the file mutex before edit_vault acquires it in the established lock order.
    let sealed = ctx.file.lock().unwrap().as_ref().ok_or(i18n::tr("Tresor ist gesperrt"))?.seal_secret(&json)?;
    let name = st.get_af_name();
    let sandbox = st.get_af_paypal_sandbox();
    ctx.edit_vault_logged("PayPal-Zugang angelegt", |v| {
        let id = v.new_id();
        v.accesses.push(BankAccess { id, name: if name.trim().is_empty() { if sandbox { "PayPal Sandbox" } else { "PayPal" }.into() } else { name.trim().into() },
            kind: AccessKind::PayPal { sandbox, credentials: sealed }, config: Default::default(), state: Default::default(), saved_pin: None });
        Ok(id)
    })
}

pub fn sync(ctx: &Ctx, access: &BankAccess, selected: Option<Vec<(Account, u32)>>) -> JobResult {
    let AccessKind::PayPal { sandbox, credentials } = &access.kind else { return Err(read_only_error()); };
    let secret = ctx.file.lock().unwrap().as_ref().ok_or(i18n::tr("Tresor ist gesperrt"))?
        .open_secret(credentials).ok_or(i18n::tr("PayPal-Zugangsdaten konnten nicht entschlüsselt werden."))?;
    let credentials = serde_json::from_str::<Credentials>(&secret).map_err(|_| i18n::tr("PayPal-Zugangsdaten konnten nicht entschlüsselt werden."))?;
    let mut client = Client::new(Http::new(), credentials, *sandbox)?;
    let selected_ids = selected.as_ref().map(|list| list.iter().map(|(_, id)| *id).collect::<HashSet<_>>());
    let currencies = selected.as_ref().map(|list| list.iter().map(|(a, _)| a.currency.clone()).collect::<BTreeSet<_>>());
    let today = Utc::now().date_naive();
    let mut from = today - Duration::days(89);
    {
        let vault = ctx.vault.lock().unwrap();
        // Re-query old pending records as well, so a delayed completion can replace them.
        for t in &vault.transactions {
            if t.tx.pending && vault.account(t.account_id).is_some_and(|a| a.access_id == access.id)
                && selected_ids.as_ref().is_none_or(|ids| ids.contains(&t.account_id)) {
                from = from.min(t.tx.booking_date);
            }
        }
    }
    ctx.bridge.status(i18n::tr("PayPal-Guthaben und Umsätze werden abgerufen …"));
    let snapshot = client.fetch(from.and_hms_opt(0, 0, 0).unwrap().and_utc(), Utc::now(), currencies.as_ref(), || ctx.bridge.cancelled.load(Ordering::Acquire))?;
    if ctx.bridge.cancelled.load(Ordering::Acquire) { return Err(i18n::tr("Vorgang abgebrochen.").into()); }
    let (accounts, added) = ctx.edit_vault(|v| apply(v, access, selected_ids.as_ref(), snapshot))?;
    Ok(i18n::tr_format!("{}: {} Konten geprüft, {} neue Umsätze.", access.name, accounts, added))
}

/// Replace only a completely retrieved interval; retain older history and manual categories.
fn apply(v: &mut Vault, access: &BankAccess, selected: Option<&HashSet<u32>>, snapshot: Snapshot) -> Result<(usize, usize), String> {
    if !v.access(access.id).is_some_and(|a| a.kind == access.kind) { return Err(i18n::tr("Bankzugang nicht gefunden").into()); }
    let currencies = if let Some(ids) = selected {
        let accounts: Vec<_> = ids.iter().map(|id| v.account(*id).filter(|a| a.access_id == access.id)).collect();
        if accounts.iter().any(|a| a.is_none()) { return Err(i18n::tr("Konto nicht gefunden").into()); }
        accounts.into_iter().flatten().map(|a| a.account.currency.clone()).collect::<BTreeSet<_>>()
    } else {
        snapshot.balances.keys().cloned().chain(snapshot.transactions.iter().map(|t| t.currency.clone()))
            .chain(v.accounts.iter().filter(|a| a.access_id == access.id).map(|a| a.account.currency.clone())).collect()
    };
    let ids = v.merge_accounts(access.id, currencies.iter().map(|currency| Account {
        account_number: format!("PAYPAL-{currency}"), sub_account: currency.clone(), currency: currency.clone(),
        product: format!("PayPal {currency}"), kind: 99, ..Default::default()
    }).collect());
    let mut added = 0;
    for (currency, id) in currencies.into_iter().zip(ids.iter().copied()) {
        let previous: HashMap<_, _> = v.transactions.iter().filter(|t| t.account_id == id)
            .map(|t| (t.tx.fingerprint.clone(), t.category.clone())).collect();
        let incoming: HashSet<_> = snapshot.transactions.iter().filter(|t| t.currency == currency).map(|t| &t.fingerprint).collect();
        v.transactions.retain(|t| t.account_id != id || (!incoming.contains(&t.tx.fingerprint) && (t.tx.booking_date < snapshot.from.date_naive() || t.tx.booking_date > snapshot.to.date_naive())));
        for tx in snapshot.transactions.iter().filter(|t| t.currency == currency) {
            if !previous.contains_key(&tx.fingerprint) { added += 1; }
            let category = previous.get(&tx.fingerprint).cloned().flatten();
            // Stable ID also removes a pending row whose booking date has since changed.
            v.transactions.push(StoredTx { account_id: id, tx: tx.clone(), category });
        }
        let account = v.accounts.iter_mut().find(|a| a.id == id).unwrap();
        account.balance = snapshot.balances.get(&currency).map(|available| Balance {
            // The API provides no booked balance. UI/export explicitly treat it as unavailable.
            booked: Decimal::ZERO, available: Some(*available), currency, date: snapshot.balance_date,
        });
        account.last_sync = Some(chrono::Local::now().naive_local());
    }
    Ok((ids.len(), added))
}

#[cfg(test)]
mod tests;
