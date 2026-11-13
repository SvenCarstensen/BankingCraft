//! Scripted bank integration tests. The transport never opens a socket.
use super::*;
use crate::syntax::Segment;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use rust_decimal::Decimal;

type Step = Box<dyn FnOnce(&[Segment]) -> Result<Vec<SegmentBuilder>> + Send>;
struct Bank { encrypted: bool, steps: VecDeque<Step>, requests: Arc<Mutex<Vec<Vec<Segment>>>> }
impl Transport for Bank {
    fn send(&mut self, raw: &[u8]) -> Result<Vec<u8>> {
        let mut card = TestDdvCard;
        let request = open_message(raw, if self.encrypted { Opener::Ddv(&mut card, 1) } else { Opener::Plain })?;
        self.requests.lock().unwrap().push(request.clone());
        let header = request.iter().find(|s| s.code == "HNHBK").unwrap();
        let msg_num = header.text_or_empty(4, 0).parse().unwrap();
        let response = self.steps.pop_front().expect("unexpected network request: no automatic payment retries")(&request)?;
        build_message(&Envelope { dialog_id: "TEST", message_number: msg_num, blz: "12345678", user_id: "fixture", system_id: "TEST" }, if self.encrypted { Auth::Ddv { card: &mut card, cid: b"TEST", sig_key: DdvKey { num: 1, version: 1 }, enc_key: DdvKey { num: 1, version: 1 } } } else { Auth::None }, response)
    }
}
#[derive(Default)]
struct Ui { verifications: Mutex<Vec<crate::vop::PayeeVerification>>, decline_vop: Mutex<bool>, answers: Mutex<VecDeque<TanAnswer>>, challenges: Mutex<Vec<TanChallenge>>, choices: Mutex<Vec<Vec<String>>> }
impl Interaction for Ui {
    fn pin(&self, _: &str) -> Option<String> { panic!("offline test must not request real credentials") }
    fn choose_tan_method(&self, m: &[TanMethod]) -> Option<String> {
        self.choices.lock().unwrap().push(m.iter().map(|m| m.security_function.clone()).collect()); m.first().map(|m| m.security_function.clone())
    }
    fn choose_tan_medium(&self, _: &[String]) -> Option<String> { None }
    fn tan(&self, c: &TanChallenge) -> TanAnswer {
        self.challenges.lock().unwrap().push(c.clone()); self.answers.lock().unwrap().pop_front().expect("unexpected TAN prompt")
    }
    fn progress(&self, _: &str) {}
    fn verify_payee(&self, result: &crate::vop::PayeeVerification, _: bool) -> bool { self.verifications.lock().unwrap().push(result.clone()); !*self.decline_vop.lock().unwrap() }
}
fn method(code: &str) -> TanMethod {
    TanMethod { security_function: code.into(), name: "Test TAN".into(), technical_id: String::new(), dk_id: String::new(), tan_process: "2".into(),
        decoupled: false, needs_medium: false, max_tan_length: 6, hhduc_response_required: false, decoupled_poll_wait_first: 0, decoupled_poll_wait_next: 0 }
}
fn setup(ui: &Ui, steps: Vec<Step>) -> (Client<'_>, Arc<Mutex<Vec<Vec<Segment>>>>) {
    let mut state = State { system_id: "TEST".into(), tan_method: Some("900".into()), ..Default::default() };
    state.bpd.vop_jobs = ["HKCCS", "HKIPZ", "HKCSE"].map(str::to_owned).to_vec();
    state.bpd.vop_formats = vec![crate::vop::REPORT_FORMAT.into()];
    state.bpd.versions.insert("HKVPP".into(), vec![1]); state.bpd.versions.insert("HKVPA".into(), vec![1]);
    state.bpd.hktan_version = 7; state.bpd.tan_methods = vec![method("900")];
    for code in ["HKCCS", "HKIPZ", "HKCSE", "HKCDE", "HKCDL", "HKCAZ", "HKCDB"] { state.bpd.versions.insert(code.into(), vec![1]); }
    state.bpd.versions.insert("HKSAL".into(), vec![5, 6, 7]); state.bpd.versions.insert("HKKAZ".into(), vec![5, 6, 7]);
    state.bpd.pain001_formats = vec!["urn:iso:std:iso:20022:tech:xsd:pain.001.001.09".into()];
    let mut client = Client::new(Config { url: "https://bank.invalid".into(), blz: "12345678".into(), ..Default::default() }, state, Medium::PinTan, ui).unwrap();
    client.in_dialog = true; client.dialog_authenticated = true; client.dialog_id = "TEST".into(); client.pin = Some(zeroize::Zeroizing::new("synthetic-pin".into()));
    let requests = Arc::new(Mutex::new(vec![])); client.set_transport(Box::new(Bank { encrypted: false, steps: steps.into(), requests: requests.clone() }));
    (client, requests)
}
fn status(code: &str) -> SegmentBuilder { SegmentBuilder::new("HIRMS", 2).deg(vec![code.into(), Val::Empty, "Fixture status".into()]) }
fn ok() -> Vec<SegmentBuilder> { vec![status("0010")] }
fn challenge(decoupled: bool) -> Vec<SegmentBuilder> {
    let mut out = vec![status("0030"), SegmentBuilder::new("HITAN", 7).de("4").de(Val::Empty).de("TASK-1").de("Bitte Testauftrag freigeben")];
    if decoupled { out.push(status("3955")); } out
}
fn segment<'a>(request: &'a [Segment], code: &str) -> &'a Segment { request.iter().find(|s| s.code == code).unwrap() }
fn account() -> Account { Account { iban: "DE89370400440532013000".into(), bic: "COBADEFFXXX".into(), account_number: "532013000".into(), currency: "EUR".into(), ..Default::default() } }
fn transfer() -> SepaTransfer {
    SepaTransfer { debtor_name: "Synthetic Sender".into(), debtor_iban: account().iban, debtor_bic: account().bic,
        creditor_name: "Synthetic Receiver".into(), creditor_iban: "DE12500105170648489890".into(), creditor_bic: "INGDDEFFXXX".into(), amount: Decimal::new(1234, 2), purpose: "Fixture only".into(), end_to_end_id: "TEST-REF".into(), execution_date: None, instant: false }
}
fn standing() -> StandingOrder {
    StandingOrder { order_id: "ORDER-123".into(), transfer: transfer(), first_date: chrono::Local::now().date_naive() + chrono::Duration::days(2),
        last_date: None, execution_time: None, interval: Interval::Monthly, every: 3, day: 99, descriptor: String::new(), raw_pain: vec![] }
}

#[test]
fn transfer_requires_sca_then_sends_only_tan_followup() {
    let ui = Ui::default(); ui.answers.lock().unwrap().push_back(TanAnswer::Tan("123456".into()));
    let (mut client, requests) = setup(&ui, vec![Box::new(|r| {
        let s = segment(r, "HKCCS"); assert_eq!(s.text_or_empty(1, 0), account().iban);
        assert_eq!(pain::read_pain001(s.bin(3, 0).unwrap()).unwrap().amount, Decimal::new(1234, 2));
        assert_eq!(segment(r, "HKTAN").text_or_empty(1, 0), "4"); Ok(matched(challenge(false)))
    }), Box::new(|r| {
        assert!(!r.iter().any(|s| s.code == "HKCCS")); let tan = segment(r, "HKTAN");
        assert_eq!(tan.text_or_empty(1, 0), "2"); assert_eq!(tan.text_or_empty(5, 0), "TASK-1"); Ok(ok())
    })]);
    assert!(client.transfer(&account(), &transfer()).is_ok()); assert_eq!(requests.lock().unwrap().len(), 2);
    assert_eq!(ui.challenges.lock().unwrap()[0].task_reference, "TASK-1");
}

#[test]
fn cancelled_payment_is_not_submitted_again() {
    let ui = Ui::default(); ui.answers.lock().unwrap().push_back(TanAnswer::Cancel);
    let (mut client, requests) = setup(&ui, vec![Box::new(|_| Ok(matched(challenge(false))))]);
    assert!(matches!(client.transfer(&account(), &transfer()), Err(Error::Cancelled))); assert_eq!(requests.lock().unwrap().len(), 1);
}

#[test]
fn decoupled_payment_polls_without_resending_the_payment() {
    let ui = Ui::default(); ui.answers.lock().unwrap().extend([TanAnswer::Confirmed, TanAnswer::Confirmed]);
    let (mut client, requests) = setup(&ui, vec![Box::new(|_| Ok(matched(challenge(true)))), Box::new(|r| {
        assert!(!r.iter().any(|s| s.code == "HKCCS")); assert_eq!(segment(r, "HKTAN").text_or_empty(1, 0), "S"); Ok(vec![status("3956")])
    }), Box::new(|r| { assert_eq!(segment(r, "HKTAN").text_or_empty(5, 0), "TASK-1"); Ok(ok()) })]);
    assert!(client.transfer(&account(), &transfer()).is_ok()); assert_eq!(requests.lock().unwrap().len(), 3);
    assert!(ui.challenges.lock().unwrap().iter().all(|c| c.decoupled));
}

#[test]
fn payment_bank_rejection_or_lost_response_never_triggers_retry() {
    for lost in [false, true] {
        let ui = Ui::default(); let (mut client, requests) = setup(&ui, vec![Box::new(move |_| {
            if lost { Err(Error::Other("simulated lost response".into())) } else { Ok(vec![status("9210")]) }
        })]);
        assert!(client.transfer(&account(), &transfer()).is_err()); assert_eq!(requests.lock().unwrap().len(), 1);
    }
}

#[test]
fn malformed_tan_challenge_fails_without_second_bank_request() {
    let ui = Ui::default(); let (mut client, requests) = setup(&ui, vec![Box::new(|_| Ok(vec![status("0030")]))]);
    assert!(client.transfer(&account(), &transfer()).is_err()); assert_eq!(requests.lock().unwrap().len(), 1);
}

#[test]
fn invalid_payment_and_schedule_inputs_make_no_bank_request() {
    let ui = Ui::default(); let (mut c, requests) = setup(&ui, vec![]);
    for amount in [Decimal::ZERO, Decimal::new(-1, 2), Decimal::new(1, 3)] { let mut t = transfer(); t.amount = amount; assert!(c.transfer(&account(), &t).is_err()); }
    let mut t = transfer(); t.creditor_iban = "invalid".into(); assert!(c.transfer(&account(), &t).is_err());
    t = transfer(); assert!(c.scheduled_transfer(&account(), &t).is_err()); t.execution_date = Some(chrono::Local::now().date_naive()); assert!(c.scheduled_transfer(&account(), &t).is_err());
    let mut so = standing(); so.every = 0; assert!(c.create_standing_order(&account(), &so).is_err());
    assert!(requests.lock().unwrap().is_empty());
}

#[test]
fn instant_transfer_uses_hkipz_and_never_falls_back_to_normal_transfer() {
    let ui = Ui::default(); let mut t = transfer(); t.instant = true;
    let (mut c, requests) = setup(&ui, vec![Box::new(|r| {
        assert!(!r.iter().any(|s| s.code == "HKCCS")); let t = pain::read_pain001(segment(r, "HKIPZ").bin(3, 0).unwrap()).unwrap(); assert!(t.instant); Ok(matched(vec![status("0010"), status("3076")]))
    })]); assert!(c.transfer(&account(), &t).is_ok()); assert_eq!(requests.lock().unwrap().len(), 1);
    c.state.bpd.versions.remove("HKIPZ"); assert!(matches!(c.transfer(&account(), &t), Err(Error::NotSupported(_)))); assert_eq!(requests.lock().unwrap().len(), 1);
}

#[test]
fn scheduled_transfer_contains_execution_date_and_returns_bank_order_id() {
    let ui = Ui::default(); let mut t = transfer(); t.execution_date = Some(chrono::Local::now().date_naive() + chrono::Duration::days(7)); let expected = t.execution_date.unwrap().to_string();
    let (mut c, _) = setup(&ui, vec![Box::new(move |r| {
        let s = segment(r, "HKCSE"); assert!(String::from_utf8_lossy(s.bin(3, 0).unwrap()).contains(&format!("<ReqdExctnDt><Dt>{expected}</Dt>")));
        let mut out = matched(vec![status("0010"), status("3076")]); out.push(SegmentBuilder::new("HICSE", 1).de("ORDER-42")); Ok(out)
    })]); assert!(c.scheduled_transfer(&account(), &t).unwrap().contains("ORDER-42"));
}

#[test]
fn standing_creation_transmits_rhythm_and_placeholder_date() {
    let ui = Ui::default(); let so = standing(); let expected = so.first_date.format("%Y%m%d").to_string();
    let (mut c, _) = setup(&ui, vec![Box::new(move |r| {
        let s = segment(r, "HKCDE"); assert_eq!(s.text_or_empty(4, 0), expected); assert_eq!(s.text_or_empty(4, 1), "M"); assert_eq!(s.text_or_empty(4, 2), "3"); assert_eq!(s.text_or_empty(4, 3), "99");
        assert!(String::from_utf8_lossy(s.bin(3, 0).unwrap()).contains("1999-01-01")); assert!(r.iter().any(|s| s.code == "HKTAN")); Ok(ok())
    })]); assert!(c.create_standing_order(&account(), &so).is_ok());
}

#[test]
fn standing_deletion_preserves_original_bank_payload_and_id() {
    let ui = Ui::default(); let mut so = standing(); so.raw_pain = b"original-bank-pain".to_vec(); so.descriptor = "bank-format".into();
    let (mut c, _) = setup(&ui, vec![Box::new(|r| {
        let s = segment(r, "HKCDL"); assert_eq!(s.text_or_empty(2, 0), "bank-format"); assert_eq!(s.bin(3, 0).unwrap(), b"original-bank-pain");
        assert_eq!(s.text_or_empty(5, 0), "ORDER-123"); assert!(r.iter().any(|s| s.code == "HKTAN")); Ok(ok())
    })]); assert!(c.delete_standing_order(&account(), &so).is_ok());
}

#[test]
fn unsupported_orders_fail_without_sending_them() {
    let ui = Ui::default(); let (mut c, requests) = setup(&ui, vec![]); c.state.bpd.versions.clear();
    let mut t = transfer(); t.execution_date = Some(chrono::Local::now().date_naive() + chrono::Duration::days(1));
    assert!(matches!(c.scheduled_transfer(&account(), &t), Err(Error::NotSupported(_))));
    assert!(matches!(c.create_standing_order(&account(), &standing()), Err(Error::NotSupported(_))));
    assert!(matches!(c.delete_standing_order(&account(), &standing()), Err(Error::NotSupported(_)))); assert!(requests.lock().unwrap().is_empty());
}

#[test]
fn tan_selection_excludes_disallowed_and_single_step_methods() {
    let ui = Ui::default(); let (mut c, _) = setup(&ui, vec![]);
    c.state.bpd.tan_methods = vec![method("999"), method("900"), method("901"), method("902")];
    c.state.allowed_methods = vec!["901".into(), "902".into()]; c.select_tan_method().unwrap(); assert_eq!(c.state.tan_method.as_deref(), Some("901"));
    assert_eq!(ui.choices.lock().unwrap()[0], vec!["901", "902"]);
    c.select_tan_method().unwrap(); assert_eq!(ui.choices.lock().unwrap().len(), 1);
    c.state.allowed_methods = vec!["902".into()]; c.select_tan_method().unwrap(); assert_eq!(c.state.tan_method.as_deref(), Some("902"));
}

#[test]
fn wrong_pin_discards_cached_pin() {
    for code in ["9931", "9942", "9340"] {
        let ui = Ui::default(); let (mut c, _) = setup(&ui, vec![Box::new(move |_| Ok(vec![status(code)]))]);
        assert!(c.balance(&account()).is_err()); assert!(c.pin.is_none());
    }
}

#[test]
fn balance_negotiates_versions_preserves_signed_amount_and_available_funds() {
    for version in [5, 6, 7] {
        let ui = Ui::default(); let (mut c, _) = setup(&ui, vec![Box::new(move |r| {
            let s = segment(r, "HKSAL"); assert_eq!(s.version, version); assert_eq!(s.text_or_empty(1, 0), if version >= 7 { account().iban } else { account().account_number });
            assert!(!r.iter().any(|s| s.code == "HKTAN")); let mut out = ok();
            out.push(SegmentBuilder::new("HISAL", version).de(Val::Empty).de("Giro").de("EUR")
                .deg(vec!["D".into(), "12,34".into(), "EUR".into(), "20261101".into()]).de(Val::Empty).de(Val::Empty).deg(vec!["123,45".into(), "EUR".into()])); Ok(out)
        })]); c.state.bpd.versions.insert("HKSAL".into(), vec![version]); c.state.bpd.tan_required.insert("HKSAL".into(), true);
        let b = c.balance(&account()).unwrap().unwrap(); assert_eq!(b.booked, Decimal::new(-1234, 2)); assert_eq!(b.available, Some(Decimal::new(12345, 2))); assert_eq!(b.currency, "EUR");
    }
}

#[test]
fn per_account_permissions_prevent_unsupported_reads() {
    let ui = Ui::default(); let (mut c, requests) = setup(&ui, vec![]); let mut a = account(); a.allowed = vec!["HKCDE".into()];
    assert!(matches!(c.balance(&a), Err(Error::NotSupported(_)))); assert!(matches!(c.transactions(&a, "2026-11-01".parse().unwrap(), None), Err(Error::NotSupported(_)))); assert!(requests.lock().unwrap().is_empty());
}

#[test]
fn credit_card_balance_dispatch_supports_standard_and_proprietary_jobs() {
    for standard in [false, true] {
        let code = if standard { "HKKKS" } else { "DKKKS" }; let version = if standard { 1 } else { 3 };
        let ui = Ui::default(); let (mut c, _) = setup(&ui, vec![Box::new(move |r| {
            let s = segment(r, code); assert_eq!(s.version, version); assert_eq!(s.text_or_empty(2, 0), "synthetic-card");
            Ok(vec![status("0010"), SegmentBuilder::new(if standard { "HIKKS" } else { "DIKKS" }, version)
                .de("synthetic-card").de(Val::Empty).deg(vec!["D".into(), "117,05".into(), "EUR".into(), "20261101".into()])])
        })]);
        let a = Account { kind: 50, account_number: "synthetic-card".into(), allowed: vec![code.into()], ..Default::default() };
        assert_eq!(c.balance(&a).unwrap().unwrap().booked, Decimal::new(-11705, 2));
    }
}

const MT940: &[u8] = b":20:TEST\r\n:60F:C261101EUR100,00\r\n:61:2611021102D12,34NTRFNONREF\r\n:86:Testzahlung\r\n:62F:C261102EUR87,66\r\n";
#[test]
fn giro_transactions_page_mt940_and_keep_mt942_pending_separate() {
    let ui = Ui::default(); let steps: Vec<Step> = (0..2).map(|page| Box::new(move |r: &[Segment]| {
        let s = segment(r, "HKKAZ"); assert_eq!(s.version, 7); assert_eq!(s.text_or_empty(3, 0), "20261101");
        assert_eq!(s.text_or_empty(6, 0), if page == 0 { "" } else { "NEXT" });
        let mut out = vec![SegmentBuilder::new("HIKAZ", 7).bin(MT940.to_vec()).bin(if page == 0 { vec![] } else { MT940.to_vec() })];
        out.push(if page == 0 { SegmentBuilder::new("HIRMS", 2).deg(vec!["3040".into(), Val::Empty, "more".into(), "NEXT".into()]) } else { status("0010") }); Ok(out)
    }) as Step).collect();
    let (mut c, requests) = setup(&ui, steps); let txs = c.transactions(&account(), "2026-11-01".parse().unwrap(), None).unwrap();
    assert_eq!(requests.lock().unwrap().len(), 2); assert_eq!(txs.len(), 3); assert_eq!(txs.iter().filter(|t| t.pending).count(), 1);
    assert!(txs.iter().all(|t| t.amount == Decimal::new(-1234, 2)));
}

#[test]
fn accounts_fill_missing_bank_code_without_network_when_upd_is_complete() {
    let ui = Ui::default(); let (mut c, requests) = setup(&ui, vec![]); c.state.upd.accounts = vec![account()];
    let accounts = c.accounts().unwrap(); assert_eq!(accounts[0].blz, "12345678"); assert!(requests.lock().unwrap().is_empty());
}

#[test]
fn ending_dialog_resets_state_and_finish_returns_persistent_state() {
    let ui = Ui::default(); let (mut c, requests) = setup(&ui, vec![Box::new(|r| { assert_eq!(segment(r, "HKEND").text_or_empty(1, 0), "TEST"); Ok(ok()) })]);
    c.end_dialog().unwrap(); assert!(!c.in_dialog); assert_eq!(c.dialog_id, "0"); assert_eq!(c.msg_num, 1);
    assert_eq!(c.finish().system_id, "TEST"); assert_eq!(requests.lock().unwrap().len(), 1);
}

#[test]
fn synchronization_and_dialog_open_follow_bank_protocol_and_reuse_system_id() {
    let ui = Ui::default(); let (mut c, requests) = setup(&ui, vec![Box::new(|r| {
        assert_eq!(segment(r, "HKIDN").text_or_empty(3, 0), "0"); assert!(r.iter().any(|s| s.code == "HKSYN"));
        assert!(!r.iter().any(|s| s.code == "HKTAN")); let mut out = ok(); out.push(SegmentBuilder::new("HISYN", 4).de("NEW-SYSTEM")); Ok(out)
    }), Box::new(|r| { segment(r, "HKEND"); Ok(ok()) }), Box::new(|r| {
        assert_eq!(segment(r, "HKIDN").text_or_empty(3, 0), "NEW-SYSTEM"); assert!(!r.iter().any(|s| s.code == "HKSYN"));
        assert_eq!(segment(r, "HKTAN").text_or_empty(2, 0), "HKIDN"); Ok(ok())
    })]);
    c.in_dialog = false; c.state.system_id.clear(); c.open().unwrap();
    assert!(c.in_dialog && c.dialog_authenticated); assert_eq!(c.state.system_id, "NEW-SYSTEM"); c.open().unwrap(); assert_eq!(requests.lock().unwrap().len(), 3);
}

#[test]
fn read_only_job_retries_with_sca_only_when_bank_explicitly_requires_it() {
    let ui = Ui::default(); let (mut c, requests) = setup(&ui, vec![Box::new(|r| {
        assert!(!r.iter().any(|s| s.code == "HKTAN")); Ok(vec![status("9075")])
    }), Box::new(|r| { assert_eq!(segment(r, "HKTAN").text_or_empty(2, 0), "HKSAL"); Ok(ok()) })]);
    c.state.bpd.tan_required.insert("HKSAL".into(), true); assert!(c.balance(&account()).is_ok()); assert_eq!(requests.lock().unwrap().len(), 2);
}

const CAMT: &[u8] = br#"<Document><BkToCstmrAcctRpt><Rpt><Ntry><Amt Ccy="EUR">12.34</Amt><CdtDbtInd>DBIT</CdtDbtInd><Sts><Cd>BOOK</Cd></Sts><BookgDt><Dt>2026-11-02</Dt></BookgDt></Ntry></Rpt></BkToCstmrAcctRpt></Document>"#;
#[test]
fn camt_dispatch_keeps_hicaz_pending_payload_pending_even_if_xml_says_booked() {
    let ui = Ui::default(); let (mut c, _) = setup(&ui, vec![Box::new(|r| {
        let s = segment(r, "HKCAZ"); assert_eq!(s.text_or_empty(4, 0), "20261101"); assert_eq!(s.text_or_empty(5, 0), "20261130");
        Ok(vec![status("0010"), SegmentBuilder::new("HICAZ", 1).de(Val::Empty).de("camt.052.001.08").bin(CAMT.to_vec()).bin(CAMT.to_vec())])
    })]); c.state.bpd.camt_formats = vec!["urn:iso:std:iso:20022:tech:xsd:camt.052.001.08".into()];
    let txs = c.transactions(&account(), "2026-11-01".parse().unwrap(), Some("2026-11-30".parse().unwrap())).unwrap();
    assert_eq!(txs.len(), 2); assert!(!txs[0].pending); assert!(txs[1].pending);
}

#[test]
fn missing_transactions_are_an_error_but_explicit_empty_result_is_valid() {
    for empty in [false, true] {
        let ui = Ui::default(); let (mut c, _) = setup(&ui, vec![Box::new(move |_| Ok(vec![status(if empty { "3010" } else { "0010" })]))]);
        let result = c.transactions(&account(), "2026-11-01".parse().unwrap(), None);
        if empty { assert!(result.unwrap().is_empty()); } else { assert!(result.is_err()); }
    }
}

#[test]
fn giro_and_standing_pages_reject_missing_or_repeated_cursor() {
    for standing_job in [false, true] { for repeat in [false, true] {
        let ui = Ui::default(); let make_step = move || Box::new(move |_: &[Segment]| {
            let mut more = vec!["3040".into(), Val::Empty, "more".into()]; if repeat { more.push("SAME".into()); }
            Ok(vec![SegmentBuilder::new("HIRMS", 2).deg(more), SegmentBuilder::new("HIKAZ", 7).bin(MT940.to_vec())])
        }) as Step;
        let (mut c, requests) = setup(&ui, vec![make_step(), make_step()]);
        let error = if standing_job { c.standing_orders(&account()).is_err() } else { c.transactions(&account(), "2026-11-01".parse().unwrap(), None).is_err() };
        assert!(error); assert_eq!(requests.lock().unwrap().len(), if repeat { 2 } else { 1 });
    }}
}

#[test]
fn incomplete_authorization_is_never_reported_as_payment_success() {
    for reply in [vec![status("0030")], vec![status("3955")], vec![status("3956")], vec![]] {
        let ui = Ui::default(); ui.answers.lock().unwrap().push_back(TanAnswer::Tan("123456".into()));
        let (mut c, requests) = setup(&ui, vec![Box::new(|_| Ok(matched(challenge(false)))), Box::new(move |_| Ok(reply))]);
        assert!(c.transfer(&account(), &transfer()).is_err()); assert_eq!(requests.lock().unwrap().len(), 2);
    }
}

#[test]
fn missing_task_reference_fails_before_asking_for_or_submitting_a_tan() {
    let ui = Ui::default(); let (mut c, requests) = setup(&ui, vec![Box::new(|_| Ok(vec![status("0030"), SegmentBuilder::new("HITAN", 7).de("4").de(Val::Empty).de(Val::Empty).de("no reference")]))]);
    assert!(c.transfer(&account(), &transfer()).is_err()); assert!(ui.challenges.lock().unwrap().is_empty()); assert_eq!(requests.lock().unwrap().len(), 1);
}

#[test]
fn missing_success_code_and_unhandled_sca_do_not_confirm_a_payment() {
    for response in [vec![], vec![status("3956")], vec![status("0030")]] {
        let ui = Ui::default(); let (mut c, requests) = setup(&ui, vec![Box::new(move |_| Ok(response))]);
        c.state.bpd.hktan_version = 0;
        assert!(c.transfer(&account(), &transfer()).is_err()); assert_eq!(requests.lock().unwrap().len(), 0);
    }
}

#[test]
fn payment_debtor_must_match_selected_account_before_any_bank_request() {
    let ui = Ui::default(); let (mut c, requests) = setup(&ui, vec![]); let mut t = transfer(); t.debtor_iban = t.creditor_iban.clone();
    assert!(c.transfer(&account(), &t).is_err()); t.execution_date = Some(chrono::Local::now().date_naive() + chrono::Duration::days(1));
    assert!(c.scheduled_transfer(&account(), &t).is_err()); let mut so = standing(); so.transfer = t;
    assert!(c.create_standing_order(&account(), &so).is_err()); assert!(requests.lock().unwrap().is_empty());
}

#[test]
fn bank_return_codes_cannot_inject_free_text_into_logs() {
    for code in ["1234\nSECRET", "PIN", "12345", "９９９９"] { assert_eq!(diagnostic_code(code), "[ungültig]"); }
    assert_eq!(diagnostic_code("0010"), "0010");
}


fn vop_result(code: &str) -> SegmentBuilder {
    SegmentBuilder::new("HIVPP", 1).bin(b"VOP-ID".to_vec()).de(Val::Empty).de(Val::Empty).de(Val::Empty).de(Val::Empty)
        .deg(vec![transfer().creditor_iban.into(), Val::Empty, if code == "RVMC" { "Synthetic Receiver GmbH".into() } else { Val::Empty }, Val::Empty, code.into(), Val::Empty])
        .de("Bank notice: please review the result.")
}
fn matched(mut response: Vec<SegmentBuilder>) -> Vec<SegmentBuilder> {
    response.push(vop_result("RCVC")); response.push(status("3091")); response
}

#[test]
fn vop_precedes_tan_and_cancel_never_authorizes_or_resends() {
    let ui = Ui::default(); *ui.decline_vop.lock().unwrap() = true;
    let (mut c, requests) = setup(&ui, vec![Box::new(|r| {
        assert_eq!(segment(r, "HKVPP").text_or_empty(1, 0), crate::vop::REPORT_FORMAT);
        assert!(!r.iter().any(|s| s.code == "HKVPA")); Ok(matched(challenge(false)))
    })]);
    assert!(matches!(c.transfer(&account(), &transfer()), Err(Error::Cancelled)));
    assert_eq!(ui.verifications.lock().unwrap().len(), 1);
    assert!(ui.challenges.lock().unwrap().is_empty()); assert_eq!(requests.lock().unwrap().len(), 1);
}

#[test]
fn vop_warnings_and_invalidated_match_require_identical_payment_and_new_tan() {
    for status_code in ["RVMC", "RVNM", "RVNA", "RCVC"] {
        let ui = Ui::default(); ui.answers.lock().unwrap().push_back(TanAnswer::Tan("123456".into()));
        let original = Arc::new(Mutex::new(Vec::new())); let sent = original.clone(); let check = original.clone();
        let (mut c, requests) = setup(&ui, vec![Box::new(move |r| {
            *sent.lock().unwrap() = segment(r, "HKCCS").bin(3, 0).unwrap().to_vec();
            Ok(vec![vop_result(status_code), status("3090"), status("3945"), status("0025")])
        }), Box::new(move |r| {
            assert_eq!(segment(r, "HKVPA").bin(1, 0).unwrap(), b"VOP-ID");
            assert_eq!(segment(r, "HKCCS").bin(3, 0).unwrap(), *check.lock().unwrap());
            assert_eq!(segment(r, "HKTAN").text_or_empty(1, 0), "4");
            assert!(!r.iter().any(|s| s.code == "HKVPP")); Ok(challenge(false))
        }), Box::new(|r| { assert!(!r.iter().any(|s| s.code == "HKCCS" || s.code == "HKVPA")); Ok(ok()) })]);
        assert!(c.transfer(&account(), &transfer()).is_ok());
        assert_eq!(ui.verifications.lock().unwrap().len(), 1); assert_eq!(ui.challenges.lock().unwrap().len(), 1);
        assert_eq!(requests.lock().unwrap().len(), 3);
    }
}

#[test]
fn unsupported_vop_never_sends_a_payment_and_missing_results_never_request_a_tan() {
    for mode in 0..4 {
        let ui = Ui::default(); let (mut c, requests) = setup(&ui, vec![]);
        match mode { 0 => { c.state.bpd.versions.remove("HKVPP"); }, 1 => c.state.bpd.vop_jobs.clear(), 2 => c.state.bpd.vop_formats.clear(), _ => { c.state.bpd.versions.insert("HKVPA".into(), vec![99]); } }
        assert!(c.transfer(&account(), &transfer()).is_err()); assert!(requests.lock().unwrap().is_empty());
    }
    for result in [vec![status("0010")], challenge(false), vec![vop_result("PDNG")], vec![vop_result("XXXX")]] {
        let ui = Ui::default(); let (mut c, requests) = setup(&ui, vec![Box::new(move |_| Ok(result))]);
        assert!(c.transfer(&account(), &transfer()).is_err()); assert!(ui.challenges.lock().unwrap().is_empty()); assert!(ui.verifications.lock().unwrap().is_empty()); assert_eq!(requests.lock().unwrap().len(), 1);
    }
}

#[test]
fn vop_polling_never_resends_payment_before_acknowledgement() {
    let ui = Ui::default(); *ui.decline_vop.lock().unwrap() = true;
    let (mut c, requests) = setup(&ui, vec![Box::new(|_| {
        Ok(vec![SegmentBuilder::new("HIVPP", 1).de(Val::Empty).de(Val::Empty).bin(b"POLL".to_vec()).de(Val::Empty).de(Val::Empty).de(Val::Empty).de("Notice from first page").de("0"),
            status("3945"), SegmentBuilder::new("HIRMS", 2).deg(vec!["3040".into(), Val::Empty, "More".into(), "CURSOR".into()])])
    }), Box::new(|r| {
        assert!(!r.iter().any(|s| s.code == "HKCCS" || s.code == "HKTAN" || s.code == "HKVPA"));
        let request = segment(r, "HKVPP"); assert_eq!(request.bin(2, 0).unwrap(), b"POLL"); assert_eq!(request.text_or_empty(4, 0), "CURSOR");
        Ok(vec![vop_result("RCVC"), status("3094")])
    })]);
    assert!(matches!(c.transfer(&account(), &transfer()), Err(Error::Cancelled)));
    assert!(ui.verifications.lock().unwrap()[0].bank_notice.contains("Notice from first page")); assert_eq!(requests.lock().unwrap().len(), 2);
}


struct TestDdvCard;
impl DdvCard for TestDdvCard {
    fn cid(&mut self) -> Result<Vec<u8>> { Ok(b"TEST".to_vec()) }
    fn bank_data(&mut self, _: u8) -> Result<crate::security::DdvBankData> { unreachable!() }
    fn keys(&mut self) -> Result<(DdvKey, DdvKey)> { Ok((DdvKey { num: 1, version: 1 }, DdvKey { num: 1, version: 1 })) }
    fn sig_id(&mut self) -> Result<u32> { Ok(1) }
    fn set_sig_id(&mut self, _: u32) -> Result<()> { Ok(()) }
    fn verify_pin(&mut self) -> Result<()> { Ok(()) }
    fn sign(&mut self, _: &[u8; 20]) -> Result<[u8; 8]> { Ok([0; 8]) }
    fn session_keys(&mut self, _: u32) -> Result<([u8; 16], [u8; 16])> { Ok(([1; 16], [1; 16])) }
    fn decrypt_key(&mut self, _: u32, _: &[u8]) -> Result<[u8; 16]> { Ok([1; 16]) }
}
#[test]
fn chip_card_vop_always_resubmits_identical_payment_with_execution_id_after_confirmation() {
    for decline in [false, true] {
        let ui = Ui::default(); *ui.decline_vop.lock().unwrap() = decline;
        let (mut c, requests) = setup(&ui, vec![]);
        c.medium = Medium::Ddv(Box::new(TestDdvCard));
        c.ddv = Some(DdvCtx { cid: b"TEST".to_vec(), sig_key: DdvKey { num: 1, version: 1 }, enc_key: DdvKey { num: 1, version: 1 } });
        let original = Arc::new(Mutex::new(Vec::new())); let saved = original.clone(); let check = original.clone();
        let mut steps: Vec<Step> = vec![Box::new(move |r| {
            assert!(!r.iter().any(|s| s.code == "HKTAN" || s.code == "HKVPA")); segment(r, "HKVPP");
            *saved.lock().unwrap() = segment(r, "HKCCS").bin(3, 0).unwrap().to_vec();
            // 3091 is a PIN/TAN shortcut and must never bypass the chip-card authorization.
            Ok(vec![vop_result("RCVC"), status("3091"), status("0025")])
        })];
        if !decline { steps.push(Box::new(move |r| {
            assert!(!r.iter().any(|s| s.code == "HKTAN" || s.code == "HKVPP"));
            assert_eq!(segment(r, "HKCCS").bin(3, 0).unwrap(), *check.lock().unwrap());
            assert_eq!(segment(r, "HKVPA").bin(1, 0).unwrap(), b"VOP-ID"); Ok(ok())
        })); }
        c.set_transport(Box::new(Bank { encrypted: true, steps: steps.into(), requests: requests.clone() }));
        let result = c.transfer(&account(), &transfer());
        if decline { assert!(matches!(result, Err(Error::Cancelled))); } else { assert!(result.is_ok(), "{result:?}"); }
        assert_eq!(requests.lock().unwrap().len(), if decline { 1 } else { 2 }); assert!(ui.challenges.lock().unwrap().is_empty());
    }
}


#[test]
fn upgrading_an_existing_bank_access_refreshes_vop_parameters_once() {
    let ui = Ui::default(); let config = Config { url: "https://bank.invalid".into(), ..Default::default() };
    let mut state = State::default(); state.bpd.version = 42;
    let client = Client::new(config.clone(), state.clone(), Medium::PinTan, &ui).unwrap();
    assert_eq!(client.state.bpd.version, 0);
    state.bpd.vop_parameters_known = true;
    let client = Client::new(config, state, Medium::PinTan, &ui).unwrap(); assert_eq!(client.state.bpd.version, 42);
}


#[test]
fn explicit_bank_waiver_is_reported_as_executed_without_resubmission() {
    let ui = Ui::default(); *ui.decline_vop.lock().unwrap() = true;
    let (mut c, requests) = setup(&ui, vec![Box::new(|_| Ok(vec![status("0010"), status("3091"), status("3076")]))]);
    assert!(c.transfer(&account(), &transfer()).is_ok());
    assert_eq!(ui.verifications.lock().unwrap()[0].status, crate::vop::PayeeStatus::Unavailable);
    assert_eq!(requests.lock().unwrap().len(), 1); assert!(ui.challenges.lock().unwrap().is_empty());
}

#[test]
fn bank_result_without_verification_never_appears_as_a_match() {
    let ui = Ui::default(); *ui.decline_vop.lock().unwrap() = true;
    let (mut c, requests) = setup(&ui, vec![Box::new(|_| Ok(vec![SegmentBuilder::new("HIVPP", 1).bin(b"notprovided".to_vec()).de(Val::Empty).de(Val::Empty).de(Val::Empty).de(Val::Empty).de(Val::Empty).de("Bank explains why verification is unavailable"), status("3945"), status("3090")]))]);
    assert!(matches!(c.transfer(&account(), &transfer()), Err(Error::Cancelled)));
    let results = ui.verifications.lock().unwrap(); assert_eq!(results[0].status, crate::vop::PayeeStatus::Unavailable); assert!(results[0].bank_notice.contains("Bank explains"));
    assert_eq!(requests.lock().unwrap().len(), 1); assert!(ui.challenges.lock().unwrap().is_empty());
}

#[test]
fn failed_execution_response_never_retries_payment_after_vop_confirmation() {
    let ui = Ui::default();
    let (mut c, requests) = setup(&ui, vec![Box::new(|_| Ok(vec![vop_result("RVNM"), status("3945")])), Box::new(|r| { segment(r, "HKVPA"); Err(Error::Transport("simulated connection loss".into())) })]);
    assert!(c.transfer(&account(), &transfer()).is_err()); assert_eq!(requests.lock().unwrap().len(), 2);
    assert_eq!(ui.verifications.lock().unwrap().len(), 1);
}
