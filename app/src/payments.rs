//! Durable outgoing orders and a separate recipient address book for each account.
use crate::store::Vault;
use chrono::{Local, NaiveDateTime};
use fints::SepaTransfer;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    Queued,
    /// Persisted BEFORE contacting the bank. After a crash this requires manual review.
    Submitting,
    Sent,
    NeedsReview,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentOrder {
    pub id: u32,
    pub account_id: u32,
    pub transfer: SepaTransfer,
    pub scheduled: bool,
    pub created_at: NaiveDateTime,
    pub status: OrderStatus,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipient {
    pub id: u32,
    pub account_id: u32,
    pub name: String,
    pub iban: String,
    pub bic: String,
}

pub fn normalize_iban(iban: &str) -> String {
    iban.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase()
}

impl Vault {
    pub fn prepare_transfer(&self, account_id: u32, mut t: SepaTransfer, scheduled: bool) -> Result<SepaTransfer, String> {
        let account = self.account(account_id).ok_or(i18n::tr("Konto nicht gefunden"))?;
        self.access(account.access_id).ok_or(i18n::tr("Bankzugang nicht gefunden"))?;
        t.debtor_iban = account.account.iban.clone();
        t.debtor_bic = account.account.bic.clone();
        t.debtor_name = if account.account.owner.is_empty() { i18n::tr("Kontoinhaber").into() } else { account.account.owner.clone() };
        t.creditor_name = t.creditor_name.trim().into();
        t.creditor_iban = normalize_iban(&t.creditor_iban);
        t.creditor_bic = t.creditor_bic.trim().to_uppercase();
        fints::pain::validate_transfer(&t).map_err(|e| e.to_string())?;
        if scheduled {
            if t.instant || t.execution_date.is_none_or(|d| d <= Local::now().date_naive()) {
                return Err(i18n::tr("Eine Terminüberweisung muss in der Zukunft liegen.").into());
            }
        } else if t.execution_date.is_some() {
            return Err(i18n::tr("Ausführungsdatum nur bei Terminüberweisungen erlaubt.").into());
        }
        Ok(t)
    }

    pub fn queue_transfer(&mut self, account_id: u32, t: SepaTransfer, scheduled: bool, save_recipient: bool) -> Result<u32, String> {
        let mut transfer = self.prepare_transfer(account_id, t, scheduled)?;
        if transfer.end_to_end_id.is_empty() { transfer.end_to_end_id = fints::pain::msg_id(); }
        if save_recipient {
            self.save_recipient(account_id, &transfer.creditor_name, &transfer.creditor_iban, &transfer.creditor_bic)?;
        }
        let id = self.new_id();
        self.payment_orders.push(PaymentOrder {
            id, account_id, transfer, scheduled, created_at: Local::now().naive_local(),
            status: OrderStatus::Queued, message: String::new(),
        });
        Ok(id)
    }

    pub fn queued_orders_for_access(&self, access_id: u32) -> Vec<u32> {
        self.payment_orders.iter().filter(|o| o.status == OrderStatus::Queued
            && self.account(o.account_id).is_some_and(|a| a.access_id == access_id))
            .map(|o| o.id).collect()
    }

    pub fn delete_payment_order(&mut self, id: u32) -> Result<(), String> {
        let index = self.payment_orders.iter().position(|o| o.id == id).ok_or(i18n::tr("Auftrag nicht gefunden"))?;
        self.payment_orders.remove(index);
        Ok(())
    }

    /// The same IBAN updates an existing recipient only within this account.
    pub fn save_recipient(&mut self, account_id: u32, name: &str, iban: &str, bic: &str) -> Result<u32, String> {
        self.account(account_id).ok_or(i18n::tr("Konto nicht gefunden"))?;
        let name = name.trim();
        let sepa_name = fints::pain::sepa_charset(name);
        if sepa_name.trim().is_empty() || sepa_name.chars().count() > 70 {
            return Err(i18n::tr("Auftraggeber und Empfänger benötigen einen Namen mit höchstens 70 SEPA-Zeichen").into());
        }
        let iban = normalize_iban(iban);
        if !fints::pain::iban_valid(&iban) { return Err(i18n::tr("Die IBAN ist ungültig.").into()); }
        let bic = bic.trim().to_uppercase();
        if let Some(r) = self.recipients.iter_mut().find(|r| r.account_id == account_id && r.iban == iban) {
            r.name = name.into();
            r.bic = bic;
            return Ok(r.id);
        }
        let id = self.new_id();
        self.recipients.push(Recipient { id, account_id, name: name.into(), iban, bic });
        Ok(id)
    }

    pub fn delete_recipient(&mut self, account_id: u32, id: u32) -> Result<(), String> {
        let index = self.recipients.iter().position(|r| r.id == id && r.account_id == account_id)
            .ok_or(i18n::tr("Empfänger nicht gefunden"))?;
        self.recipients.remove(index);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Vault, u32, u32) {
        let mut v = Vault::default();
        crate::demo::add_demo(&mut v);
        let a = v.accounts[0].id;
        let b = v.accounts[1].id;
        (v, a, b)
    }
    #[test]
    fn address_book_requires_opt_in_and_is_isolated_per_account() {
        let (mut v, a, b) = fixture();
        let mut t = crate::business_tests::standing().transfer;
        v.queue_transfer(a, t.clone(), false, false).unwrap();
        assert!(v.recipients.is_empty());
        v.queue_transfer(a, t.clone(), false, true).unwrap();
        assert_eq!(v.recipients.len(), 1);
        let id = v.recipients[0].id;
        t.creditor_name = "Changed without consent".into();
        v.queue_transfer(a, t.clone(), false, false).unwrap();
        assert_eq!(v.recipients[0].name, "Empfänger");
        t.creditor_iban = "de12 5001 0517 0648 4898 90".into();
        t.creditor_name = "Updated".into();
        v.queue_transfer(a, t.clone(), false, true).unwrap();
        assert_eq!(v.recipients.len(), 1);
        assert_eq!(v.recipients[0].id, id);
        assert_eq!(v.recipients[0].name, "Updated");
        v.save_recipient(b, "Other account", &t.creditor_iban, "testdeff").unwrap();
        assert_eq!(v.recipients.len(), 2);
        assert!(v.delete_recipient(b, id).is_err());
        v.delete_recipient(a, id).unwrap();
        assert_eq!(v.recipients[0].account_id, b);
        assert_eq!(v.recipients[0].bic, "TESTDEFF");
        assert_eq!(v.payment_orders[0].transfer.creditor_name, "Empfänger", "orders are snapshots, not live contacts");
    }
    #[test]
    fn invalid_payment_neither_queues_nor_saves_recipient() {
        let (mut v, a, _) = fixture();
        let t = crate::business_tests::standing().transfer;
        let before = serde_json::to_vec(&v).unwrap();
        let mut invalid = t.clone(); invalid.amount = rust_decimal::Decimal::ZERO;
        assert!(v.queue_transfer(a, invalid, false, true).is_err());
        assert!(v.queue_transfer(999, t.clone(), false, true).is_err());
        let mut invalid = t.clone(); invalid.execution_date = Some(Local::now().date_naive());
        assert!(v.queue_transfer(a, invalid, true, true).is_err());
        assert_eq!(serde_json::to_vec(&v).unwrap(), before);
        assert!(v.save_recipient(a, "", &t.creditor_iban, "").is_err());
        assert!(v.save_recipient(a, "Recipient", "DE00000000000000000000", "").is_err());
    }
    #[test]
    fn old_vaults_and_new_data_roundtrip_and_access_deletion_are_supported() {
        let (mut v, a, _) = fixture();
        let mut old = serde_json::to_value(&v).unwrap();
        old.as_object_mut().unwrap().remove("payment_orders");
        old.as_object_mut().unwrap().remove("recipients");
        let old: Vault = serde_json::from_value(old).unwrap();
        assert!(old.payment_orders.is_empty() && old.recipients.is_empty());
        let id = v.queue_transfer(a, crate::business_tests::standing().transfer, false, true).unwrap();
        let mut other = v.accesses[0].clone(); other.id = v.new_id();
        let access = other.id; v.accesses.push(other);
        let mut account = v.accounts[0].account.clone(); account.account_number = "other".into();
        let b = v.merge_accounts(access, vec![account])[0];
        v.queue_transfer(b, crate::business_tests::standing().transfer, false, true).unwrap();
        let mut loaded: Vault = serde_json::from_slice(&serde_json::to_vec(&v).unwrap()).unwrap();
        assert_eq!(loaded.queued_orders_for_access(v.account(a).unwrap().access_id), vec![id]);
        assert_eq!(loaded.payment_orders[0].transfer, v.payment_orders[0].transfer);
        loaded.remove_access(v.account(a).unwrap().access_id);
        assert_eq!(loaded.payment_orders.len(), 1);
        assert_eq!(loaded.payment_orders[0].account_id, b);
        assert_eq!(loaded.recipients.len(), 1);
        assert_eq!(loaded.recipients[0].account_id, b);
    }
}
