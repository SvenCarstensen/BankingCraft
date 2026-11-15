use super::*;
use crate::{store::{SealedSecret, VaultFile}, ui::Model, view::{self, ViewState}};
use chrono::{DateTime, Utc};
fn time(s: &str) -> DateTime<Utc> { s.parse().unwrap() }
fn access() -> BankAccess { BankAccess { id: 1, name: "Synthetic PayPal".into(), kind: AccessKind::PayPal { sandbox: true, credentials: SealedSecret { nonce: vec![0; 12], data: vec![] } }, config: Default::default(), state: Default::default(), saved_pin: None } }
fn snapshot(pending: bool) -> Snapshot {
    Snapshot { from: time("2026-11-01T00:00:00Z"), to: time("2026-11-14T00:00:00Z"), balance_date: time("2026-11-14T00:00:00Z").date_naive(),
        balances: [("EUR".into(), Decimal::from(90)), ("USD".into(), Decimal::from(42))].into(),
        transactions: vec![banking::Transaction { booking_date: time("2026-11-02T00:00:00Z").date_naive(), value_date: time("2026-11-02T00:00:00Z").date_naive(), amount: Decimal::from(-10), currency: "EUR".into(), fingerprint: "paypal:TX01:EUR:Payment".into(), end_to_end_id: "TX01".into(), pending, ..Default::default() }] }
}
fn setup() -> (Vault, BankAccess) { let access = access(); let vault = Vault { next_id: 1, accesses: vec![access.clone()], ..Default::default() }; (vault, access) }
#[test]
fn refresh_updates_pending_in_place_retains_categories_and_older_history() {
    let (mut v, access) = setup();
    assert_eq!(apply(&mut v, &access, None, snapshot(true)).unwrap(), (2, 1));
    let account_id = v.transactions[0].account_id;
    v.transactions[0].category = Some("Shopping".into());
    let mut old = v.transactions[0].clone(); old.tx.fingerprint = "older".into(); old.tx.booking_date = time("2026-08-01T00:00:00Z").date_naive(); old.tx.pending = false;
    v.transactions.push(old);
    let mut next = snapshot(false); next.transactions[0].booking_date += Duration::days(1); next.transactions[0].amount = Decimal::from(-11);
    assert_eq!(apply(&mut v, &access, None, next).unwrap(), (2, 0));
    assert_eq!(v.transactions.len(), 2);
    let latest = v.transactions.iter().find(|t| t.tx.end_to_end_id == "TX01" && t.tx.fingerprint != "older").unwrap();
    assert_eq!(latest.account_id, account_id); assert!(!latest.tx.pending); assert_eq!(latest.category.as_deref(), Some("Shopping")); assert_eq!(latest.tx.amount, Decimal::from(-11));
    assert!(v.accounts.iter().all(|a| !a.account.is_sepa()));
    assert!(v.prepare_transfer(account_id, crate::business_tests::standing().transfer, false).unwrap_err().contains("PayPal"));
}
#[test]
fn selected_wallet_does_not_mutate_other_currencies_and_clears_removed_pending() {
    let (mut v, access) = setup(); apply(&mut v, &access, None, snapshot(true)).unwrap();
    let euro = v.accounts.iter().find(|a| a.account.currency == "EUR").unwrap().id;
    let dollar = v.accounts.iter().find(|a| a.account.currency == "USD").unwrap().clone();
    let mut next = snapshot(false); next.transactions.clear(); next.balances.insert("USD".into(), Decimal::from(999));
    assert_eq!(apply(&mut v, &access, Some(&HashSet::from([euro])), next).unwrap(), (1, 0));
    assert!(v.transactions.is_empty());
    assert_eq!(serde_json::to_vec(v.account(dollar.id).unwrap()).unwrap(), serde_json::to_vec(&dollar).unwrap());
}
#[test]
fn available_balance_is_not_added_to_pending_or_exported_as_booked() {
    i18n::with_language(i18n::Language::German, || {
    let (mut v, access) = setup(); apply(&mut v, &access, None, snapshot(true)).unwrap();
    let app = AppWindow::new().unwrap(); let mut state = ViewState::default();
    view::refresh_all(&app, &v, &mut state);
    let st = app.global::<AppState>();
    let eur = st.get_accounts().row_data(0).unwrap();
    assert!(eur.paypal); assert_eq!(eur.balance, "90,00 €"); assert_eq!(eur.booked_balance, "–");
    assert_eq!(st.get_total_booked_balance(), "–\n–");
    assert!(st.get_total_has_paypal()); assert_eq!(st.get_sepa_accounts().row_count(), 0);
    let xml = crate::export::camt052_content(&v, &[0]);
    assert!(xml.contains("<Cd>CLAV</Cd>")); assert!(!xml.contains("CLBD")); assert!(xml.contains("<Othr><Id>PAYPAL-EUR</Id></Othr>"));
    assert!(!xml.contains("<IBAN></IBAN></Id><Ccy>"));
    });
}
#[test]
fn encrypted_credentials_survive_password_change_without_plaintext_on_disk() {
    let path = std::env::temp_dir().join(format!("bankingcraft-paypal-{:016x}.vault", rand::random::<u64>()));
    let mut file = VaultFile::create(path.clone(), "old-fixture-password").unwrap();
    let (mut v, mut access) = setup();
    let credential_json = r#"{"user":"synthetic-api-user","password":"synthetic-api-password","signature":"synthetic-api-signature"}"#;
    access.kind = AccessKind::PayPal { sandbox: true, credentials: file.seal_secret(credential_json).unwrap() };
    v.accesses[0] = access;
    file.save(&v).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(!bytes.windows(b"synthetic-api".len()).any(|x| x == b"synthetic-api"));
    let before = serde_json::to_vec(&v).unwrap();
    assert!(file.change_password("wrong", "new-fixture-password", &mut v).is_err());
    assert_eq!(before, serde_json::to_vec(&v).unwrap());
    file.change_password("old-fixture-password", "new-fixture-password", &mut v).unwrap();
    let AccessKind::PayPal { credentials, .. } = &v.accesses[0].kind else { panic!() };
    assert_eq!(&**file.open_secret(credentials).unwrap(), credential_json);
    assert!(VaultFile::open(path.clone(), "old-fixture-password").is_err());
    let (new_file, mut loaded) = VaultFile::open(path.clone(), "new-fixture-password").unwrap();
    let AccessKind::PayPal { credentials, .. } = &mut loaded.accesses[0].kind else { panic!() };
    credentials.data[0] ^= 1;
    let disk = std::fs::read(&path).unwrap(); let old = serde_json::to_vec(&loaded).unwrap();
    let mut new_file = new_file;
    assert!(new_file.change_password("new-fixture-password", "another-password", &mut loaded).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), disk); assert_eq!(serde_json::to_vec(&loaded).unwrap(), old);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn foreign_currency_does_not_enter_euro_analytics_or_transaction_totals() {
    i18n::with_language(i18n::Language::German, || {
        let (mut v, access) = setup();
        let mut data = snapshot(false);
        let today = chrono::Local::now().date_naive();
        data.from = (today - Duration::days(3)).and_hms_opt(0, 0, 0).unwrap().and_utc();
        data.to = Utc::now();
        data.transactions[0].booking_date = today;
        let mut dollar = data.transactions[0].clone(); dollar.currency = "USD".into(); dollar.amount = Decimal::from(900); dollar.fingerprint = "paypal:TX02:USD:Payment".into(); dollar.end_to_end_id = "TX02".into();
        data.transactions.push(dollar);
        apply(&mut v, &access, None, data).unwrap();
        let app = AppWindow::new().unwrap(); let mut state = ViewState::default();
        view::refresh_all(&app, &v, &mut state);
        let st = app.global::<AppState>();
        assert_eq!(st.get_month_income(), "0,00 €"); assert_eq!(st.get_month_expense(), "10,00 €");
        assert!(st.get_tx_summary().contains("900,00 USD")); assert!(!st.get_tx_summary().contains("900,00 €"));
        assert_eq!(st.get_analysis_income().row_count(), 0);
        st.set_tx_account(2);
        view::refresh_transactions(&app, &v, &state);
        let header = st.get_tx_rows().row_data(0).unwrap();
        assert!(header.day_balance.contains("900,00 USD")); assert!(!header.day_balance.contains("Gebuchter"));
    });
}

#[test]
#[ignore = "explicit synthetic balance screenshots; no network"]
fn render_paypal_balances() {
    i18n::with_language(i18n::Language::German, || {
        let (mut v, access) = setup(); apply(&mut v, &access, None, snapshot(true)).unwrap();
        crate::demo::add_demo(&mut v);
        let app = AppWindow::new().unwrap(); let st = app.global::<AppState>();
        st.set_unlocked(true); st.set_theme_mode(2);
        view::refresh_all(&app, &v, &mut ViewState::default());
        for page in [0, 1, 2] {
            st.set_page(page);
            crate::ui::testing::snapshot(&app, 1100, 1200, std::env::temp_dir().join(format!("bankingcraft-paypal-balances-{page}.png")));
        }
    });
}
