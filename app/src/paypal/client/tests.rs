use super::*;
use std::collections::VecDeque;

fn time(s: &str) -> DateTime<Utc> { s.parse().unwrap() }
fn bounds() -> (DateTime<Utc>, DateTime<Utc>) { (time("2026-11-01T00:00:00Z"), time("2026-11-03T00:00:00Z")) }
fn credentials() -> Credentials { Credentials { user: "test+&=user".into(), password: "test+&=password".into(), signature: "test+&=signature".into() } }
fn response(fields: Fields) -> Vec<u8> { url::form_urlencoded::Serializer::new(String::new()).extend_pairs(fields).finish().into_bytes() }
fn balance() -> Vec<u8> { b"ACK=Success&TIMESTAMP=2026-11-03T00%3A00%3A00Z&L_AMT0=90.25&L_CURRENCYCODE0=EUR&L_AMT1=42.00&L_CURRENCYCODE1=USD".to_vec() }
fn rows(spec: &[(&str, &str, &str, &str)]) -> Fields {
    let mut fields = Fields::from([("ACK".into(), "Success".into())]);
    for (i, (id, at, currency, status)) in spec.iter().enumerate() {
        for (k, v) in [("L_TRANSACTIONID", *id), ("L_TIMESTAMP", *at), ("L_CURRENCYCODE", *currency), ("L_STATUS", *status),
            ("L_NETAMT", "97.00"), ("L_AMT", "100.00"), ("L_FEEAMT", "-3.00"), ("L_TYPE", "Payment"), ("L_NAME", "Fictional Café + Shop")] {
            fields.insert(format!("{k}{i}"), v.into());
        }
    }
    fields
}
#[derive(Default)]
struct Mock { responses: VecDeque<Result<Vec<u8>>>, requests: Vec<(String, Fields)> }
impl Transport for Mock {
    fn post(&mut self, endpoint: &str, body: &str) -> Result<Vec<u8>> {
        self.requests.push((endpoint.into(), parse(body.as_bytes()).unwrap()));
        self.responses.pop_front().expect("unexpected request")
    }
}
fn client(responses: Vec<Vec<u8>>) -> Client<Mock> {
    Client::new(Mock { responses: responses.into_iter().map(Ok).collect(), ..Default::default() }, credentials(), false).unwrap()
}
#[test]
fn readonly_credentials_encoding_net_fees_and_separate_currency_balances() {
    let (from, to) = bounds();
    let mut c = client(vec![response(rows(&[("TX01", "2026-11-02T00:00:00Z", "EUR", "Completed")])), balance()]);
    let snapshot = c.fetch(from, to, None, || false).unwrap();
    assert_eq!(snapshot.transactions[0].amount, Decimal::from(97));
    assert_eq!(snapshot.transactions[0].counterparty_name, "Fictional Café + Shop");
    assert_eq!(snapshot.balances["EUR"], Decimal::new(9025, 2));
    assert_eq!(snapshot.balances["USD"], Decimal::from(42));
    assert_eq!(c.transport.requests.len(), 2);
    for (endpoint, request) in &c.transport.requests {
        assert_eq!(endpoint, LIVE);
        assert_eq!(request["USER"], credentials().user);
        assert_eq!(request["PWD"], credentials().password);
        assert_eq!(request["SIGNATURE"], credentials().signature);
        assert_eq!(request["VERSION"], "204.0");
        assert!(matches!(request["METHOD"].as_str(), "GetBalance" | "TransactionSearch"));
    }
    assert_eq!(c.transport.requests[1].1["RETURNALLCURRENCIES"], "1");
    assert!(c.request("DoDirectPayment", &[], &|| false).is_err());
    assert_eq!(c.transport.requests.len(), 2);
}
#[test]
fn truncation_warning_splits_window_and_deduplicates_inclusive_boundary() {
    let (from, to) = bounds();
    let row = ("TX01", "2026-11-02T00:00:00Z", "EUR", "Completed");
    let warning = b"ACK=SuccessWithWarning&L_ERRORCODE0=11002".to_vec();
    let mut c = client(vec![warning, response(rows(&[row])), response(rows(&[row])), balance()]);
    let snapshot = c.fetch(from, to, None, || false).unwrap();
    assert_eq!(snapshot.transactions.len(), 1);
    assert_eq!(c.transport.requests[1].1["ENDDATE"], c.transport.requests[2].1["STARTDATE"]);
}
#[test]
fn hundred_rows_without_warning_are_also_split_and_same_second_overflow_fails() {
    let (from, to) = bounds();
    let mut full = rows(&[]);
    for i in 0..100 { full.insert(format!("L_TRANSACTIONID{i}"), format!("TX{i}")); }
    let encoded = response(full);
    let mut c = client(vec![encoded.clone(), response(rows(&[])), response(rows(&[])), balance()]);
    c.fetch(from, to, None, || false).unwrap();
    assert_eq!(c.transport.requests.len(), 4);
    let mut c = client(vec![encoded]);
    assert!(c.fetch(from, from + Duration::seconds(1), None, || false).is_err());
    assert_eq!(c.transport.requests.len(), 1);
}
#[test]
fn scoped_currency_and_sandbox_never_query_another_wallet_or_live_endpoint() {
    let (from, to) = bounds();
    let mut c = client(vec![response(rows(&[])), balance()]); c.sandbox = true;
    c.fetch(from, to, Some(&BTreeSet::from(["EUR".into()])), || false).unwrap();
    assert_eq!(c.transport.requests[0].1["CURRENCYCODE"], "EUR");
    assert!(c.transport.requests.iter().all(|(endpoint, _)| endpoint == SANDBOX));
}
#[test]
fn failures_unknown_warnings_and_cancellation_do_not_return_partial_snapshot_or_remote_text() {
    let (from, to) = bounds();
    let mut c = client(vec![b"ACK=Failure&L_ERRORCODE0=10002&L_LONGMESSAGE0=private-token".to_vec()]);
    let error = c.fetch(from, to, None, || false).unwrap_err();
    assert!(error.contains("10002")); assert!(!error.contains("private-token"));
    for wire in ["ACK=SuccessWithWarning&L_ERRORCODE0=99999", "ACK=SuccessWithWarning", "ACK=Failure&L_ERRORCODE0=private-token"] {
        let mut c = client(vec![wire.as_bytes().to_vec()]);
        assert!(!c.fetch(from, to, None, || false).unwrap_err().contains("private-token"));
    }
    let mut c = client(vec![]);
    assert!(c.fetch(from, to, None, || true).is_err());
    assert!(c.transport.requests.is_empty());
    let mut c = client(vec![response(rows(&[]))]);
    c.transport.responses.push_back(Err("synthetic network failure".into()));
    assert!(c.fetch(from, to, None, || false).is_err());
    let mut c = client(vec![]); c.calls = MAX_CALLS;
    assert!(c.fetch(from, to, None, || false).is_err());
    assert!(c.transport.requests.is_empty());
}
#[test]
fn invalid_records_are_rejected_instead_of_silently_disappearing() {
    let (from, to) = bounds();
    for (field, value) in [("L_NETAMT0", "oops"), ("L_TIMESTAMP0", "tomorrow"), ("L_TIMESTAMP0", "2026-11-05T00:00:00Z"),
        ("L_CURRENCYCODE0", "EU"), ("L_STATUS0", "Unrecognized"), ("L_TRANSACTIONID0", ""), ("L_TRANSACTIONID1", "TX02")] {
        let mut data = rows(&[("TX01", "2026-11-02T00:00:00Z", "EUR", "Completed")]);
        data.insert(field.into(), value.into());
        let mut c = client(vec![response(data)]);
        assert!(c.fetch(from, to, None, || false).is_err(), "{field}");
    }
    for wire in ["ACK=Success&ACK=Failure", "ACK=Success&L_NAME0=missing-id", "ACK=Success&L_TRANSACTIONID2=gap", "ACK=Success&X=%ZZ", "ACK=Success&X=%FF", "ACK=Success&X=%", "ACK"] {
        let mut c = client(vec![wire.as_bytes().to_vec()]);
        assert!(c.fetch(from, to, None, || false).is_err(), "{wire}");
    }
}
#[test]
fn denied_rows_are_omitted_pending_rows_and_signed_net_are_preserved() {
    let (from, to) = bounds();
    let mut data = rows(&[("TX01", "2026-11-02T00:00:00Z", "EUR", "Pending"), ("TX02", "2026-11-02T00:00:00Z", "EUR", "Denied")]);
    data.insert("L_NETAMT0".into(), "-25.50".into());
    let mut c = client(vec![response(data), balance()]);
    let snapshot = c.fetch(from, to, None, || false).unwrap();
    assert_eq!(snapshot.transactions.len(), 1); assert!(snapshot.transactions[0].pending);
    assert_eq!(snapshot.transactions[0].amount, Decimal::new(-2550, 2));
}
#[test]
fn changing_duplicate_across_pages_is_not_silently_accepted() {
    let (from, to) = bounds();
    let one = rows(&[("TX01", "2026-11-02T00:00:00Z", "EUR", "Completed")]);
    let mut two = one.clone(); two.insert("L_NETAMT0".into(), "98.00".into());
    let mut c = client(vec![b"ACK=SuccessWithWarning&L_ERRORCODE0=11002".to_vec(), response(one), response(two)]);
    assert!(c.fetch(from, to, None, || false).is_err());
}
