//! Scoped refresh regression tests: synthetic accounts and a transport without sockets.
use super::*;
use fints::{security::{Auth, Envelope, Opener, build_message, open_message}, syntax::{SegmentBuilder, Val}};

fn context() -> Ctx {
    Ctx { vault: Arc::new(Mutex::new(Vault::default())), file: Arc::new(Mutex::new(None)),
        bridge: Bridge::new(slint::Weak::default()), running: Arc::new(AtomicBool::new(false)) }
}

#[test]
fn selected_demo_refresh_leaves_queued_payments_and_other_accounts_untouched() {
    let ctx = context();
    assert!(sync_account(&ctx, 999).is_err());
    let id = {
        let mut v = ctx.vault.lock().unwrap();
        demo::add_demo(&mut v);
        let id = v.accounts[1].id;
        v.queue_transfer(id, crate::business_tests::standing().transfer, false, false).unwrap();
        id
    };
    let before = serde_json::to_vec(&*ctx.vault.lock().unwrap()).unwrap();
    assert!(sync_account(&ctx, id).unwrap().contains("Demo"));
    assert_eq!(serde_json::to_vec(&*ctx.vault.lock().unwrap()).unwrap(), before);
}

struct Ui;
impl fints::Interaction for Ui {
    fn pin(&self, _: &str) -> Option<String> { Some("synthetic-pin".into()) }
    fn choose_tan_method(&self, _: &[TanMethod]) -> Option<String> { panic!("no TAN selection") }
    fn choose_tan_medium(&self, _: &[String]) -> Option<String> { panic!("no TAN medium") }
    fn tan(&self, _: &TanChallenge) -> fints::TanAnswer { panic!("no payments or approval") }
    fn progress(&self, _: &str) {}
}

struct Bank { numbers: Vec<String>, requests: Arc<Mutex<Vec<String>>> }
impl fints::transport::Transport for Bank {
    fn send(&mut self, raw: &[u8]) -> fints::Result<Vec<u8>> {
        let request = open_message(raw, Opener::Plain)?;
        let message_number = request.iter().find(|s| s.code == "HNHBK").unwrap().text_or_empty(4, 0).parse().unwrap();
        let mut response = vec![SegmentBuilder::new("HIRMG", 2).deg(vec!["0010".into(), Val::Empty, "OK".into()])];
        for segment in request.iter().filter(|s| s.code.starts_with("HK")) {
            self.requests.lock().unwrap().push(if matches!(segment.code.as_str(), "HKSAL" | "HKKAZ") {
                format!("{}:{}", segment.code, segment.text_or_empty(1, 0))
            } else { segment.code.clone() });
            match segment.code.as_str() {
                "HKIDN" | "HKVVB" | "HKEND" => {},
                "HKSAL" | "HKKAZ" => {
                    let number = segment.text_or_empty(1, 0);
                    assert!(self.numbers.contains(&number), "request must address only selected accounts");
                    if segment.code == "HKSAL" {
                        response.push(SegmentBuilder::new("HISAL", 5).de(number).de("Selected")
                            .de("EUR").deg(vec!["C".into(), "123,00".into(), "EUR".into(), "20261113".into()]));
                    } else {
                        response.push(SegmentBuilder::new("HIKAZ", 5).bin(
                            b":20:SCOPED\n:25:12345678/4711099\n:28C:1/1\n:60F:C261112EUR121,77\n:61:2611131113C1,23NTRFSCOPED\n:86:Synthetic scoped refresh\n:62F:C261113EUR123,00\n".to_vec()));
                    }
                },
                code => panic!("single-account refresh must not discover accounts or submit orders: {code}"),
            }
        }
        build_message(&Envelope { dialog_id: "TEST", message_number, blz: "12345678", user_id: "synthetic", system_id: "TEST" }, Auth::None, response)
    }
}

#[test]
fn selected_refresh_requests_only_its_balance_and_transactions_and_never_sends_payments() {
    verify_scope(false);
}

#[test]
fn group_refresh_uses_one_dialog_for_its_accounts_and_leaves_other_accounts_and_payments_untouched() {
    verify_scope(true);
}

fn verify_scope(group: bool) {
    let ctx = context();
    let id = {
        let mut v = ctx.vault.lock().unwrap();
        demo::add_demo(&mut v);
        v.accesses[0].kind = AccessKind::PinTan;
        // Accidental payment dispatch must fail before opening a connection.
        v.accesses[0].config.url = "http://bank.invalid".into();
        let id = v.accounts[1].id;
        v.accounts[1].display_name = "Selected test account".into();
        v.accounts[1].account.allowed = vec!["HKSAL".into(), "HKKAZ".into()];
        let mut other_access = v.accesses[0].clone();
        other_access.id = v.new_id();
        other_access.name = "Other bank".into();
        let mut other_account = v.accounts[0].clone();
        other_account.id = v.new_id();
        other_account.access_id = other_access.id;
        v.accesses.push(other_access);
        v.accounts.push(other_account);
        let sibling = v.accounts[0].id;
        for account_id in [id, sibling] {
            v.queue_transfer(account_id, crate::business_tests::standing().transfer, false, false).unwrap();
        }
        id
    };
    let (account, access) = account_and_access(&ctx, id).unwrap();
    let selected = if group {
        let group_id = {
            let mut v = ctx.vault.lock().unwrap();
            let group_id = v.create_group("Test group").unwrap();
            let sibling = v.accounts[0].id;
            v.assign_group(id, Some(group_id)).unwrap();
            v.assign_group(sibling, Some(group_id)).unwrap();
            group_id
        };
        let mut targets = group_targets(&ctx, group_id).unwrap();
        assert_eq!(targets.len(), 1);
        targets.remove(0).accounts
    } else { vec![(account, id)] };
    let ids: Vec<_> = selected.iter().map(|(_, id)| *id).collect();
    let numbers: Vec<_> = selected.iter().map(|(a, _)| a.account_number.clone()).collect();
    let before = ctx.vault.lock().unwrap().clone();
    let mut state = fints::State { system_id: "TEST".into(), ..Default::default() };
    state.bpd.tan_methods.push(TanMethod { security_function: "999".into(), name: "Test".into(), technical_id: String::new(),
        dk_id: String::new(), tan_process: "1".into(), decoupled: false, needs_medium: false, max_tan_length: 0,
        hhduc_response_required: false, decoupled_poll_wait_first: 0, decoupled_poll_wait_next: 0 });
    state.bpd.versions.insert("HKSAL".into(), vec![5]);
    state.bpd.versions.insert("HKKAZ".into(), vec![5]);
    let mut client = Client::new(fints::Config { url: "https://bank.invalid".into(), blz: "12345678".into(), ..Default::default() },
        state, Medium::PinTan, &Ui).unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    client.set_transport(Box::new(Bank { numbers: numbers.clone(), requests: requests.clone() }));
    let message = sync_with_client(&ctx, &access, Some(selected), client).unwrap();
    if !group { assert!(message.contains("Selected test account")); }
    let mut expected: Vec<String> = vec!["HKIDN".into(), "HKVVB".into()];
    for number in numbers { expected.extend([format!("HKSAL:{number}"), format!("HKKAZ:{number}")]); }
    expected.push("HKEND".into());
    assert_eq!(*requests.lock().unwrap(), expected);
    let after = ctx.vault.lock().unwrap();
    for id in &ids {
        assert_eq!(after.account(*id).unwrap().balance.as_ref().unwrap().booked, rust_decimal::Decimal::from(123));
        assert!(after.account(*id).unwrap().last_sync.is_some());
        assert_eq!(after.transactions.iter().filter(|t| t.account_id == *id && !t.tx.pending).count(),
            before.transactions.iter().filter(|t| t.account_id == *id && !t.tx.pending).count() + 1);
    }
    // A complete report without pending entries replaces the selected accounts'
    // old pending entries; booked transactions and other accounts must remain.
    assert!(!after.transactions.iter().any(|t| ids.contains(&t.account_id) && t.tx.pending));
    let untouched_before: Vec<_> = before.transactions.iter().filter(|t| !ids.contains(&t.account_id)).collect();
    let untouched_after: Vec<_> = after.transactions.iter().filter(|t| !ids.contains(&t.account_id)).collect();
    assert_eq!(serde_json::to_vec(&untouched_after).unwrap(), serde_json::to_vec(&untouched_before).unwrap());
    for old in before.accounts.iter().filter(|a| !ids.contains(&a.id)) {
        assert_eq!(serde_json::to_vec(after.account(old.id).unwrap()).unwrap(), serde_json::to_vec(old).unwrap());
    }
    assert_eq!(serde_json::to_vec(&after.payment_orders).unwrap(), serde_json::to_vec(&before.payment_orders).unwrap());
    assert_eq!(serde_json::to_vec(&after.accesses[1]).unwrap(), serde_json::to_vec(&before.accesses[1]).unwrap());
}

#[test]
fn group_targets_split_by_bank_handle_ungrouped_and_never_expand_invalid_or_empty_groups() {
    let ctx = context();
    let (group_id, first, same_bank, other_bank, ungrouped, empty) = {
        let mut v = ctx.vault.lock().unwrap();
        demo::add_demo(&mut v);
        let first = v.accounts[0].id;
        let ungrouped = v.accounts[1].id;
        let mut same = v.accounts[0].clone();
        same.id = v.new_id();
        let same_bank = same.id;
        v.accounts.push(same);
        let mut access = v.accesses[0].clone();
        access.id = v.new_id();
        let mut other = v.accounts[0].clone();
        other.id = v.new_id();
        other.access_id = access.id;
        let other_bank = other.id;
        v.accesses.push(access);
        v.accounts.push(other);
        let group_id = v.create_group("Selected group").unwrap();
        let empty = v.create_group("Empty group").unwrap();
        for id in [first, same_bank, other_bank] { v.assign_group(id, Some(group_id)).unwrap(); }
        v.queue_transfer(first, crate::business_tests::standing().transfer, false, false).unwrap();
        (group_id, first, same_bank, other_bank, ungrouped, empty)
    };
    let targets = group_targets(&ctx, group_id).unwrap();
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].accounts.iter().map(|(_, id)| *id).collect::<Vec<_>>(), [first, same_bank]);
    assert_eq!(targets[1].accounts.iter().map(|(_, id)| *id).collect::<Vec<_>>(), [other_bank]);
    let targets = group_targets(&ctx, 0).unwrap();
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].accounts.iter().map(|(_, id)| *id).collect::<Vec<_>>(), [ungrouped]);
    assert!(group_targets(&ctx, 99999).is_err());
    assert!(group_targets(&ctx, empty).is_err());
    let before = serde_json::to_vec(&*ctx.vault.lock().unwrap()).unwrap();
    sync_group(&ctx, group_id).unwrap(); // demo path also must not send queued orders
    assert_eq!(serde_json::to_vec(&*ctx.vault.lock().unwrap()).unwrap(), before);
}
