//! Offline business scenarios; only synthetic data and isolated temporary vaults.
use crate::{categorize::categorize, export, store::*, view};
use chrono::{Duration, NaiveDate};
use banking::{Account, Balance, Interval, SepaTransfer, StandingOrder, Transaction};
use rust_decimal::Decimal;

pub(crate) fn date(s: &str) -> NaiveDate { s.parse().unwrap() }
pub(crate) fn tx(amount: i64, day: &str) -> Transaction {
    let mut tx = Transaction { amount: Decimal::new(amount, 2), booking_date: date(day), value_date: date(day),
        currency: "EUR".into(), counterparty_name: "Testgeschäft".into(), purpose: "Testkauf".into(), ..Default::default() };
    tx.compute_fingerprint(); tx
}
pub(crate) fn standing() -> StandingOrder {
    StandingOrder { order_id: "TEST-ORDER".into(), transfer: SepaTransfer {
        debtor_name: "Test".into(), debtor_iban: "DE89370400440532013000".into(), debtor_bic: String::new(),
        creditor_name: "Empfänger".into(), creditor_iban: "DE12500105170648489890".into(), creditor_bic: String::new(),
        amount: Decimal::new(1234, 2), purpose: "Test".into(), end_to_end_id: String::new(), execution_date: None, instant: false,
    }, first_date: date("2026-01-31"), last_date: None, execution_time: None, interval: Interval::Monthly,
        every: 1, day: 31, raw_pain: vec![], descriptor: String::new() }
}
fn account(number: &str) -> Account {
    Account { account_number: number.into(), blz: "37040044".into(), currency: "EUR".into(), ..Default::default() }
}
struct Temp(std::path::PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("bankingcraft-scenario-{:016x}", rand::random::<u64>()));
        std::fs::create_dir(&p).unwrap(); Self(p)
    }
}
impl Drop for Temp { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }

#[test]
fn account_refresh_enriches_iban_and_preserves_identity_balance_group_and_order() {
    let mut v = Vault::default();
    let ids = v.merge_accounts(10, vec![account("123"), account("456")]);
    let g = v.create_group("Privat").unwrap(); v.assign_group(ids[0], Some(g)).unwrap();
    v.accounts[0].balance = Some(Balance { booked: Decimal::new(-1234, 2), available: None, currency: "EUR".into(), date: date("2026-11-01") });
    v.accounts[0].last_sync = Some(date("2026-11-01").and_hms_opt(12, 0, 0).unwrap());
    v.move_account(ids[1], ids[0], false).unwrap();
    let mut enriched = account("123"); enriched.iban = "DE89370400440532013000".into(); enriched.product = "Giro".into();
    assert_eq!(v.merge_accounts(10, vec![enriched.clone()]), vec![ids[0]]);
    assert_eq!(v.accounts.iter().map(|a| a.id).collect::<Vec<_>>(), vec![ids[1], ids[0]]);
    assert_eq!(v.account(ids[0]).unwrap().balance.as_ref().unwrap().booked, Decimal::new(-1234, 2));
    assert!(v.account(ids[0]).unwrap().last_sync.is_some());
    assert_eq!(v.groups[0].account_ids, vec![ids[0]]);
    assert_ne!(v.merge_accounts(11, vec![enriched])[0], ids[0], "separate bank access");
}

#[test]
fn subaccounts_are_separate_even_when_account_numbers_match() {
    let mut v = Vault::default(); let a = account("123"); let mut b = a.clone(); b.sub_account = "01".into();
    let ids = v.merge_accounts(1, vec![a, b]); assert_ne!(ids[0], ids[1]); assert_eq!(v.accounts.len(), 2);
}

#[test]
fn repeated_and_overlapping_fetches_keep_duplicate_bookings_and_manual_categories() {
    let mut v = Vault::default(); let t = tx(-100, "2026-11-01");
    assert_eq!(v.merge_transactions(1, vec![t.clone(), t.clone()]), 2);
    v.transactions[0].category = Some("Wohnen".into());
    assert_eq!(v.merge_transactions(1, vec![t.clone(), t.clone()]), 0);
    assert_eq!(v.merge_transactions(1, vec![t.clone(), t.clone(), t.clone()]), 1);
    assert_eq!(v.merge_transactions(2, vec![t]), 1);
    assert_eq!(v.transactions.len(), 4); assert_eq!(v.transactions[0].category.as_deref(), Some("Wohnen"));
}

#[test]
fn pending_to_booked_replaces_only_pending_entries_of_the_fetched_account() {
    let mut v = Vault::default(); let mut pending = tx(-100, "2026-11-01"); pending.pending = true;
    assert_eq!(v.merge_transactions(1, vec![pending.clone()]), 0);
    v.merge_transactions(2, vec![pending]);
    assert_eq!(v.merge_transactions(1, vec![tx(-100, "2026-11-01")]), 1);
    assert_eq!(v.transactions.iter().filter(|t| t.tx.pending).count(), 1);
    assert_eq!(v.merge_transactions(1, vec![]), 0); assert_eq!(v.transactions.len(), 2);
    v.merge_transactions(2, vec![]); assert_eq!(v.transactions.len(), 1);
}

#[test]
fn identical_pending_entry_does_not_inflate_booked_duplicate_count() {
    let mut v = Vault::default(); let booked = tx(-100, "2026-11-01");
    let mut pending = booked.clone(); pending.pending = true;
    v.merge_transactions(1, vec![booked.clone()]);
    for _ in 0..3 { assert_eq!(v.merge_transactions(1, vec![pending.clone(), booked.clone()]), 0); }
    assert_eq!(v.transactions.len(), 2);
}

#[test]
fn fetch_start_ignores_pending_unknown_dates_and_other_accounts() {
    let mut v = Vault::default();
    assert_eq!(v.last_booking(1), None);
    let mut unknown = tx(-100, "2099-12-31"); unknown.date_unknown = true;
    let mut pending = tx(-200, "2099-12-30"); pending.pending = true;
    v.merge_transactions(1, vec![tx(-300, "2026-11-01"), unknown, pending]);
    v.merge_transactions(2, vec![tx(-300, "2099-12-29")]);
    assert_eq!(v.last_booking(1), Some(date("2026-11-01")));
}

#[test]
fn deleting_bank_access_cascades_without_touching_another_access() {
    let mut v = Vault::default(); let a = v.merge_accounts(10, vec![account("1")])[0]; let b = v.merge_accounts(20, vec![account("2")])[0];
    for (id, aid) in [(10, a), (20, b)] {
        v.accesses.push(BankAccess { id, name: "Fixture".into(), kind: AccessKind::Demo, config: Default::default(), state: Default::default(), saved_pin: None });
        v.merge_transactions(aid, vec![tx(-100, "2026-11-01")]); v.standing.push(StoredStanding { account_id: aid, order: standing() });
    }
    let g = v.create_group("Gemischt").unwrap(); v.assign_group(a, Some(g)).unwrap(); v.assign_group(b, Some(g)).unwrap();
    v.remove_access(10); v.remove_access(999);
    assert!(v.access(10).is_none()); assert!(v.account(a).is_none()); assert!(v.access(20).is_some());
    assert_eq!(v.transactions.len(), 1); assert_eq!(v.transactions[0].account_id, b);
    assert_eq!(v.standing.len(), 1); assert_eq!(v.standing[0].account_id, b); assert_eq!(v.groups[0].account_ids, vec![b]);
}

#[test]
fn invalid_group_edits_and_drag_targets_leave_the_vault_unchanged() {
    let mut v = Vault::default(); let a = v.merge_accounts(1, vec![account("1")])[0]; let g = v.create_group("Privat").unwrap();
    v.assign_group(a, Some(g)).unwrap(); let before = serde_json::to_vec(&v).unwrap();
    for name in ["", "  ", "OHNE GRUPPE", "PRIVAT", "bad\nname", &"a".repeat(61)] { assert!(v.create_group(name).is_err()); }
    assert!(v.assign_group(a, Some(999)).is_err()); assert!(v.assign_group(999, Some(g)).is_err());
    assert!(v.rename_group(999, "Neu").is_err()); assert!(v.delete_group(999).is_err());
    assert!(v.move_account(a, 999, true).is_err()); assert!(v.move_account(999, a, false).is_err());
    v.move_account(a, a, true).unwrap(); assert_eq!(serde_json::to_vec(&v).unwrap(), before);
}

#[test]
fn assigning_the_same_group_twice_and_unassigning_are_idempotent() {
    let mut v = Vault::default(); let a = v.merge_accounts(1, vec![account("1")])[0]; let g = v.create_group("  Privat  ").unwrap();
    for _ in 0..2 { v.assign_group(a, Some(g)).unwrap(); }
    assert_eq!(v.groups[0].account_ids, vec![a]); assert_eq!(v.groups[0].name, "Privat");
    for _ in 0..2 { v.assign_group(a, None).unwrap(); } assert!(v.groups[0].account_ids.is_empty());
    v.rename_group(g, "PRIVAT").unwrap(); assert_eq!(v.groups[0].name, "PRIVAT");
}

#[test]
fn vault_roundtrip_encrypts_data_uses_fresh_nonces_and_restricts_file_permissions() {
    let temp = Temp::new(); let path = temp.0.join("test.vault");
    let f = VaultFile::create(path.clone(), "synthetic-test-password").unwrap(); let mut v = Vault::default();
    v.create_group("CONFIDENTIAL-FIXTURE-GROUP").unwrap(); f.save(&v).unwrap(); let first = std::fs::read(&path).unwrap();
    f.save(&v).unwrap(); let second = std::fs::read(&path).unwrap(); assert_ne!(first, second);
    assert!(!String::from_utf8_lossy(&second).contains("CONFIDENTIAL-FIXTURE-GROUP"));
    assert_eq!(VaultFile::open(path.clone(), "synthetic-test-password").unwrap().1.groups[0].name, "CONFIDENTIAL-FIXTURE-GROUP");
    assert!(VaultFile::open(path.clone(), "wrong").is_err());
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600); }
    assert_eq!(std::fs::read_dir(&temp.0).unwrap().count(), 1);
}

#[test]
fn vault_rejects_modified_truncated_and_unknown_files() {
    let temp = Temp::new(); let path = temp.0.join("test.vault");
    VaultFile::create(path.clone(), "test").unwrap().save(&Vault::default()).unwrap(); let valid = std::fs::read(&path).unwrap();
    for len in [0, 7, 35, valid.len()-1] { std::fs::write(&path, &valid[..len]).unwrap(); assert!(VaultFile::open(path.clone(), "test").is_err()); }
    let mut modified = valid; *modified.last_mut().unwrap() ^= 1; std::fs::write(&path, modified).unwrap();
    assert!(VaultFile::open(path, "test").is_err());
}

#[test]
fn saved_pin_is_bound_to_its_vault_and_rejects_tampering() {
    let temp = Temp::new(); let a = VaultFile::create(temp.0.join("a"), "test").unwrap(); let b = VaultFile::create(temp.0.join("b"), "test").unwrap();
    let mut secret = a.seal_secret("synthetic-pin").unwrap(); assert_eq!(&**a.open_secret(&secret).as_ref().unwrap(), "synthetic-pin");
    assert!(b.open_secret(&secret).is_none()); secret.data[0] ^= 1; assert!(a.open_secret(&secret).is_none());
    secret.nonce.clear(); assert!(a.open_secret(&secret).is_none());
}

#[test]
fn failed_vault_replace_removes_temporary_file() {
    let temp = Temp::new(); let path = temp.0.join("directory-not-a-file"); std::fs::create_dir(&path).unwrap();
    let f = VaultFile::create(path.clone(), "test").unwrap(); assert!(f.save(&Vault::default()).is_err());
    assert!(path.is_dir()); assert_eq!(std::fs::read_dir(&temp.0).unwrap().count(), 1);
}

#[test]
fn categories_prioritize_manual_then_learned_then_builtin_and_fallback() {
    let mut t = StoredTx { account_id: 1, tx: tx(-100, "2026-11-01"), category: None }; t.tx.counterparty_name = "REWE".into();
    assert_eq!(categorize(&t, &[]), "Lebensmittel");
    let rules = vec![Rule { pattern: "rewe".into(), category: "Freizeit".into() }]; assert_eq!(categorize(&t, &rules), "Freizeit");
    t.category = Some("Wohnen".into()); assert_eq!(categorize(&t, &rules), "Wohnen");
    t.category = None; t.tx.counterparty_name = "REWE Nebenbetrieb".into(); assert_eq!(categorize(&t, &rules), "Lebensmittel");
    t.tx.counterparty_name = "Unbekannt".into(); t.tx.purpose = "Gehalt".into(); assert_eq!(categorize(&t, &[]), "Sonstiges");
    t.tx.amount = Decimal::ONE; assert_eq!(categorize(&t, &[]), "Gehalt"); t.tx.purpose.clear(); assert_eq!(categorize(&t, &[]), "Einnahmen");
}

#[test]
fn csv_preserves_selected_order_quotes_and_unknown_dates() {
    let mut v = Vault::default(); let mut a = tx(-1234, "2026-11-01"); a.counterparty_name = "Test;\"Shop\"\rFiliale".into(); a.date_unknown = true; a.pending = true;
    v.merge_transactions(1, vec![a, tx(200, "2026-11-02")]);
    let csv = export::csv_content(&v, &[1, 999, 0]);
    assert!(csv.starts_with('\u{feff}')); assert!(csv.contains("\"Test;\"\"Shop\"\"\rFiliale\""));
    assert!(!csv.contains("01.11.2026")); assert!(csv.contains("-12,34;EUR;vorgemerkt\r\n"));
    assert!(csv.find("02.11.2026").unwrap() < csv.find("Test;").unwrap());
}

#[test]
fn csv_neutralizes_formula_payloads_in_every_untrusted_column_and_keeps_amount_numeric() {
    for payload in ["=1+2", "+1+2", "-1+2", "@SUM(A1)", "\t=1+2", "\r\n=1+2", "  =1+2", "\u{feff}=1+2", "＝1+2", "=1+2\";=1+2"] {
        let mut v = Vault::default(); let mut t = tx(-1234, "2026-11-01");
        t.counterparty_name = payload.into(); t.purpose = payload.into(); t.counterparty_iban = payload.into(); t.currency = payload.into();
        v.merge_transactions(1, vec![t]); let csv = export::csv_content(&v, &[0]);
        assert!(csv.contains(&format!("\"\t{}\"", payload.replace('"', "\"\""))), "{payload:?}");
        assert!(csv.contains(";-12,34;"), "trusted Decimal must remain numeric");
    }
}

#[test]
fn oversize_vault_is_rejected_before_allocating_or_decrypting() {
    let temp = Temp::new(); let path = temp.0.join("oversize.vault");
    let file = std::fs::File::create(&path).unwrap(); file.set_len(256 * 1024 * 1024 + 1).unwrap();
    assert!(VaultFile::open(path, "synthetic-test-password").err().unwrap().contains("zu groß"));
}

#[test]
fn camt_export_roundtrips_debit_credit_pending_references_and_xml_characters() {
    let mut v = Vault::default(); let mut a = account("1"); a.iban = "DE89370400440532013000".into(); let id = v.merge_accounts(1, vec![a])[0];
    let mut debit = tx(-1234, "2026-11-01"); debit.counterparty_name = "A & B <Test>".into(); debit.purpose = "A&B <invoice>".into(); debit.mandate_id = "MANDATE&1".into(); debit.end_to_end_id = "REF&1".into();
    let mut pending = tx(-300, "2026-11-02"); pending.pending = true; pending.date_unknown = true;
    v.merge_transactions(id, vec![debit.clone(), tx(500, "2026-11-02"), pending]);
    let xml = export::camt052_content(&v, &[0, 1, 2, 999]);
    let parsed=roxmltree::Document::parse(&xml).unwrap();
    let entries=parsed.descendants().filter(|n|n.has_tag_name("Ntry")).collect::<Vec<_>>();
    assert_eq!(entries.len(),3);
    let text=|n:roxmltree::Node<'_, '_>,tag:&str|n.descendants().find(|n|n.has_tag_name(tag)).and_then(|n|n.text()).unwrap_or("").to_owned();
    assert_eq!(text(entries[0],"Amt"),"12.34"); assert_eq!(text(entries[0],"CdtDbtInd"),"DBIT");
    assert_eq!(text(entries[0],"Nm"),debit.counterparty_name); assert_eq!(text(entries[0],"Ustrd"),debit.purpose);
    assert_eq!(text(entries[0],"EndToEndId"),debit.end_to_end_id); assert_eq!(text(entries[0],"MndtId"),debit.mandate_id);
    assert_eq!(text(entries[1],"Amt"),"5.00"); assert_eq!(text(entries[1],"CdtDbtInd"),"CRDT");
    assert_eq!(text(entries[2],"Cd"),"PDNG");
    assert!(!entries[2].descendants().any(|n|n.has_tag_name("BookgDt")));

}

#[test]
fn standing_month_end_leap_year_and_ultimo_offsets() {
    let mut o = standing();
    assert_eq!(view::next_execution(&o, date("2026-02-01")), Some(date("2026-02-28")));
    o.first_date = date("2028-01-31"); assert_eq!(view::next_execution(&o, date("2028-02-01")), Some(date("2028-02-29")));
    for (day, expected) in [(97, "2028-02-27"), (98, "2028-02-28"), (99, "2028-02-29")] {
        o.day = day; assert_eq!(view::next_execution(&o, date("2028-02-01")), Some(date(expected)));
    }
}

#[test]
fn standing_weekly_quarterly_and_end_date_boundaries() {
    let mut o = standing(); o.interval = Interval::Weekly; o.every = 2; o.first_date = date("2026-01-05"); o.day = 1;
    assert_eq!(view::next_execution(&o, o.first_date + Duration::days(1)), Some(date("2026-01-19")));
    o.last_date = Some(date("2026-01-19")); assert_eq!(view::next_execution(&o, date("2026-01-19")), o.last_date);
    assert_eq!(view::next_execution(&o, date("2026-01-20")), None);
    o = standing(); o.every = 3; assert_eq!(view::next_execution(&o, date("2026-02-01")), Some(date("2026-04-30")));
}

#[test]
fn date_and_amount_entry_and_german_display_cover_boundaries() {
    assert_eq!(view::parse_date("29.02.2028"), Some(date("2028-02-29"))); assert_eq!(view::parse_date("2028-02-29"), Some(date("2028-02-29")));
    for s in ["29.02.2027", "31.04.2026", "bad"] { assert!(view::parse_date(s).is_none()); }
    for s in ["1.234,56 €", "1234.56", "1 234,56"] { assert_eq!(view::parse_amount(s), Some(Decimal::new(123456, 2))); }
    for s in ["1,234", "NaN", "", "--1"] { assert!(view::parse_amount(s).is_none()); }
    assert_eq!(view::fmt_money(Decimal::new(-123456789, 2), "EUR"), "−1.234.567,89 €");
    assert_eq!(view::fmt_money(Decimal::ZERO, "USD"), "0,00 USD");
}
