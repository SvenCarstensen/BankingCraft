//! Scoped refresh regression tests: synthetic accounts and a backend double without sockets.
use super::*;

fn context() -> Ctx {
    Ctx { vault: Arc::new(Mutex::new(Vault::default())), file: Arc::new(Mutex::new(None)),
        bridge: Bridge::new(crate::ui::Weak::default()), running: Arc::new(AtomicBool::new(false)) }
}

#[test]
fn selected_demo_refresh_leaves_other_accounts_orders_untouched() {
    let ctx = context();
    assert!(sync_account(&ctx, 999).is_err());
    let id = {
        let mut v = ctx.vault.lock().unwrap();
        demo::add_demo(&mut v);
        let id = v.accounts[1].id;
        let other = v.accounts[0].id;
        v.queue_transfer(other, crate::business_tests::standing().transfer, false, false).unwrap();
        id
    };
    let before = serde_json::to_vec(&*ctx.vault.lock().unwrap()).unwrap();
    assert!(sync_account(&ctx, id).unwrap().contains("Demo"));
    assert_eq!(serde_json::to_vec(&*ctx.vault.lock().unwrap()).unwrap(), before);
}

struct Bank { numbers: Vec<String>, requests: Arc<Mutex<Vec<String>>> }

#[test]
fn refresh_progress_tracks_account_and_operation_despite_native_messages() {
    use banking::Interaction;
    struct ProgressBank {
        bank: Bank,
        accounts: Vec<Account>,
        bridge: Bridge,
        app: crate::AppWindow,
        observed: Arc<Mutex<Vec<String>>>,
    }
    impl ProgressBank {
        fn observe(&self) {
            // Real native jobs emit many progress callbacks during each call.
            self.bridge.progress("Receiving data");
            self.bridge.progress("Using GnuTLS default ciphers.");
            self.app.pump(&egui::Context::default());
            self.observed.lock().unwrap().push(self.app.global::<AppState>().get_busy_text().to_string());
        }
    }
    impl RefreshClient for ProgressBank {
        fn accounts(&mut self) -> banking::Result<Vec<Account>> { self.observe(); Ok(self.accounts.clone()) }
        fn current_accounts(&mut self, accounts: Vec<Account>) -> banking::Result<Vec<Account>> { self.observe(); Ok(accounts) }
        fn balance(&mut self, a: &Account) -> banking::Result<Option<banking::Balance>> { self.observe(); self.bank.balance(a) }
        fn transaction_report(&mut self, a: &Account, f: chrono::NaiveDate, t: Option<chrono::NaiveDate>) -> banking::Result<banking::TransactionReport> {
            self.observe(); self.bank.transaction_report(a, f, t)
        }
        fn finish(self) -> banking::State { self.bank.finish() }
    }
    i18n::with_language(i18n::Language::German, || {
        for scoped in [false, true] {
            let app=crate::AppWindow::new().unwrap();
            let mut ctx=context(); ctx.bridge=Bridge::new(app.as_weak());
            let (access,selected)={
                let mut v=ctx.vault.lock().unwrap(); demo::add_demo(&mut v);
                v.accesses[0].name="Testbank".into();
                let access=v.accesses[0].clone();
                let selected=v.accounts.iter_mut().filter(|a| a.access_id==access.id).take(2).enumerate().map(|(i,a)| {
                    a.display_name=if i==0 {"Giro privat"} else {""}.into();
                    a.account.product="Sparkonto".into(); a.account.kind=1;
                    a.account.allowed=vec!["HKSAL".into(),"HKKAZ".into()];
                    (a.account.clone(),a.id)
                }).collect::<Vec<_>>();
                (access,selected)
            };
            assert_eq!(selected.len(),2);
            let observed=Arc::new(Mutex::new(Vec::new()));
            let bridge=ctx.bridge.with_application_progress();
            let bank=Bank {numbers:selected.iter().map(|(a,_)|a.account_number.clone()).collect(),requests:Default::default()};
            let client=ProgressBank {bank,accounts:selected.iter().map(|(a,_)|a.clone()).collect(),bridge:bridge.clone(),app:app.clone(),observed:observed.clone()};
            sync_with_client(&ctx,&access,scoped.then_some(selected),client).unwrap();
            assert_eq!(*observed.lock().unwrap(),[
                "Testbank: Kontodaten aktualisieren …",
                "Giro privat · Testbank: Saldo abrufen …",
                "Giro privat · Testbank: Umsätze abrufen …",
                "Sparkonto · Testbank: Saldo abrufen …",
                "Sparkonto · Testbank: Umsätze abrufen …",
            ]);
            // Suppression belongs only to this client, not to later jobs.
            ctx.bridge.progress("Other job"); app.pump(&egui::Context::default());
            assert_eq!(app.global::<AppState>().get_busy_text(),"Other job");
            ctx.bridge.cancelled.store(true,Ordering::Release);
            assert!(bridge.cancelled());
        }
    });
}

impl RefreshClient for Bank {
    fn accounts(&mut self) -> banking::Result<Vec<Account>> { panic!("scoped refresh must not discover accounts") }
    fn balance(&mut self, account:&Account)->banking::Result<Option<banking::Balance>> {
        assert!(self.numbers.contains(&account.account_number));
        self.requests.lock().unwrap().push(format!("balance:{}",account.account_number));
        Ok(Some(banking::Balance {booked:123.into(),available:None,currency:"EUR".into(),date:chrono::NaiveDate::from_ymd_opt(2026,11,13).unwrap()}))
    }
    fn transaction_report(&mut self,account:&Account,_:chrono::NaiveDate,_:Option<chrono::NaiveDate>)->banking::Result<banking::TransactionReport> {
        assert!(self.numbers.contains(&account.account_number));
        self.requests.lock().unwrap().push(format!("transactions:{}",account.account_number));
        let date=chrono::NaiveDate::from_ymd_opt(2026,11,13).unwrap();
        let mut transaction=banking::Transaction {booking_date:date,value_date:date,amount:rust_decimal::Decimal::new(123,2),
            currency:"EUR".into(),purpose:"Synthetic scoped refresh".into(),..Default::default()};
        transaction.compute_fingerprint();
        Ok(banking::TransactionReport {transactions:vec![transaction],..Default::default()})
    }
    fn finish(self)->banking::State {Default::default()}
}

struct FailedAccess {
    bank: Bank,
    failure: Option<Error>,
    fail_balance: bool,
}

struct CurrentCapabilities {
    bank: Bank,
    current: Vec<Account>,
}
impl RefreshClient for CurrentCapabilities {
    fn accounts(&mut self) -> banking::Result<Vec<Account>> {
        self.bank.requests.lock().unwrap().push("discovery".into());
        Ok(self.current.clone())
    }
    fn current_accounts(&mut self, old: Vec<Account>) -> banking::Result<Vec<Account>> {
        self.bank.requests.lock().unwrap().push("capabilities".into());
        assert_eq!(old.len(),self.current.len());
        Ok(self.current.clone())
    }
    fn balance(&mut self,a:&Account)->banking::Result<Option<banking::Balance>> { self.bank.balance(a) }
    fn transaction_report(&mut self,a:&Account,f:chrono::NaiveDate,t:Option<chrono::NaiveDate>)->banking::Result<banking::TransactionReport> {
        assert!(a.kind!=50,"unsupported card job must not be sent");
        self.bank.transaction_report(a,f,t)
    }
    fn finish(self)->banking::State { self.bank.finish() }
}

#[test]
fn refresh_uses_current_card_capabilities_preserves_history_and_counts_only_updated_accounts() {
    for scoped in [false,true] {
        for balance_supported in [false,true] {
            let ctx=context();
            let (access,selected,before)={
                let mut v=ctx.vault.lock().unwrap(); demo::add_demo(&mut v);
                v.accesses[0].kind=AccessKind::PinTan;
                let access=v.accesses[0].clone();
                let selected=v.accounts.iter_mut().filter(|a| a.access_id==access.id).take(2).map(|a| {
                    a.account.kind=1; a.account.allowed=vec!["HKSAL".into(),"HKKAZ".into()];
                    a.last_sync=None; (a.account.clone(),a.id)
                }).collect::<Vec<_>>();
                (access,selected,v.clone())
            };
            assert_eq!(selected.len(),2);
            let requests=Arc::new(Mutex::new(Vec::new()));
            let mut current=selected.iter().map(|(a,_)|a.clone()).collect::<Vec<_>>();
            current[0].kind=50;
            current[0].allowed=vec![if balance_supported {"HKSAL"} else {"AQBANKING_NO_SUPPORTED_JOBS"}.into()];
            let bank=Bank {numbers:current.iter().map(|a|a.account_number.clone()).collect(),requests:requests.clone()};
            let message=sync_with_client(&ctx,&access,scoped.then(||selected.clone()),CurrentCapabilities {bank,current}).unwrap();
            let mut expected=vec![if scoped {"capabilities"} else {"discovery"}.into()];
            if balance_supported { expected.push(format!("balance:{}",selected[0].0.account_number)); }
            expected.extend([format!("balance:{}",selected[1].0.account_number),format!("transactions:{}",selected[1].0.account_number)]);
            assert_eq!(*requests.lock().unwrap(),expected);
            assert!(message.contains("kein unterstützter Umsatzabruf"));
            assert!(message.contains(&format!("{} Konten aktualisiert",if balance_supported {2} else {1})));
            let after=ctx.vault.lock().unwrap(); let card=selected[0].1;
            assert!(!after.account(card).unwrap().account.allows("HKKAZ"));
            let before_tx=before.transactions.iter().filter(|t|t.account_id==card).collect::<Vec<_>>();
            let after_tx=after.transactions.iter().filter(|t|t.account_id==card).collect::<Vec<_>>();
            assert_eq!(serde_json::to_vec(&before_tx).unwrap(),serde_json::to_vec(&after_tx).unwrap());
            if !balance_supported { assert!(after.account(card).unwrap().last_sync.is_none()); }
        }
    }
}

#[test]
fn unavailable_statements_are_explicit_and_do_not_clear_history_or_hide_bank_failures() {
    for unavailable in [false,true] {
        let ctx=context();
        let (access,selected,before)={
            let mut v=ctx.vault.lock().unwrap(); demo::add_demo(&mut v);
            v.accesses[0].kind=AccessKind::PinTan;
            let access=v.accesses[0].clone();
            let a=v.accounts.iter_mut().find(|a|a.access_id==access.id).unwrap();
            a.account.kind=50; a.account.allowed=vec!["HKSAL".into(),"HKKAZ".into()]; a.last_sync=None;
            let selected=vec![(a.account.clone(),a.id)];
            (access,selected,v.clone())
        };
        let requests=Arc::new(Mutex::new(Vec::new()));
        let bank=Bank {numbers:vec![selected[0].0.account_number.clone()],requests};
        let failure=if unavailable {Error::NotSupported("Synthetic unavailable statement job".into())}
            else {Error::Bank {code:"9010".into(),text:"Synthetic bank rejection".into()}};
        let result=sync_with_client(&ctx,&access,Some(selected.clone()),FailedAccess {bank,failure:Some(failure),fail_balance:false});
        if unavailable { assert!(result.unwrap().contains("Synthetic unavailable statement job")); }
        else { assert!(result.unwrap_err().contains("Synthetic bank rejection")); }
        let after=ctx.vault.lock().unwrap();
        assert_eq!(serde_json::to_vec(&after.transactions).unwrap(),serde_json::to_vec(&before.transactions).unwrap());
        assert!(after.account(selected[0].1).unwrap().last_sync.is_none());
        assert_eq!(after.account(selected[0].1).unwrap().balance.as_ref().unwrap().booked,rust_decimal::Decimal::from(123));
    }
}
impl RefreshClient for FailedAccess {
    fn accounts(&mut self) -> banking::Result<Vec<Account>> { panic!("scoped refresh") }
    fn balance(&mut self, account:&Account)->banking::Result<Option<banking::Balance>> {
        if self.fail_balance {
            self.bank.requests.lock().unwrap().push(format!("balance:{}",account.account_number));
            Err(self.failure.take().expect("failed access must not be retried"))
        } else { self.bank.balance(account) }
    }
    fn transaction_report(&mut self,account:&Account,_:chrono::NaiveDate,_:Option<chrono::NaiveDate>)->banking::Result<banking::TransactionReport> {
        self.bank.requests.lock().unwrap().push(format!("transactions:{}",account.account_number));
        Err(self.failure.take().expect("failed access must not be retried"))
    }
    fn finish(self)->banking::State {
        banking::State { aqbanking:Some(banking::AqBankingState {version:1,binding:"synthetic".into(),
            snapshot:b"updated native state after failure".to_vec(),accounts:Vec::new()}) }
    }
}

#[test]
fn broken_access_stops_after_first_failure_and_preserves_partial_results_and_native_state() {
    for fail_balance in [true,false] {
        for failure in [Error::Access {code:"3".into(),text:"Synthetic setup failure".into()},
            Error::Transport("synthetic".into()),Error::Security("synthetic".into()),
            Error::Bank{code:"9930".into(),text:"Synthetic rejected PIN".into()},Error::Cancelled] {
            let ctx=context();
            let (access,selected,before)={
                let mut vault=ctx.vault.lock().unwrap(); demo::add_demo(&mut vault);
                vault.accesses[0].kind=AccessKind::PinTan;
                let access=vault.accesses[0].clone();
                let selected=vault.accounts.iter_mut().filter(|a| a.access_id==access.id).take(2).map(|a| {
                    a.account.kind=1; a.account.allowed=vec!["HKSAL".into(),"HKKAZ".into()];
                    a.last_sync=None; (a.account.clone(),a.id)
                }).collect::<Vec<_>>();
                assert_eq!(selected.len(),2);
                (access,selected,vault.clone())
            };
            let requests=Arc::new(Mutex::new(Vec::new()));
            let bank=Bank {numbers:selected.iter().map(|(a,_)| a.account_number.clone()).collect(),requests:requests.clone()};
            let client=FailedAccess {bank,failure:Some(failure),fail_balance};
            let message=sync_with_client(&ctx,&access,Some(selected.clone()),client).unwrap_err();
            assert!(!message.contains("Konten geprüft"));
            let number=&selected[0].0.account_number;
            let mut expected=vec![format!("balance:{number}")];
            if !fail_balance {expected.push(format!("transactions:{number}"));}
            assert_eq!(*requests.lock().unwrap(),expected);
            let after=ctx.vault.lock().unwrap();
            assert_eq!(after.access(access.id).unwrap().state.aqbanking.as_ref().unwrap().snapshot,b"updated native state after failure");
            assert_eq!(serde_json::to_vec(&after.transactions).unwrap(),serde_json::to_vec(&before.transactions).unwrap());
            assert_eq!(serde_json::to_vec(&after.payment_orders).unwrap(),serde_json::to_vec(&before.payment_orders).unwrap());
            assert!(after.account(selected[0].1).unwrap().last_sync.is_none());
            if !fail_balance {assert_eq!(after.account(selected[0].1).unwrap().balance.as_ref().unwrap().booked,rust_decimal::Decimal::from(123));}
            let untouched=selected[1].1;
            assert_eq!(serde_json::to_vec(after.account(untouched).unwrap()).unwrap(),serde_json::to_vec(before.account(untouched).unwrap()).unwrap());
        }
    }
}

#[test]
fn selected_refresh_requests_only_its_balance_and_transactions_and_never_sends_payments() {
    verify_scope(false);
}

#[test]
fn group_refresh_uses_one_client_for_its_accounts_and_leaves_other_accounts_and_payments_untouched() {
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
        targets.remove(0).accounts.unwrap()
    } else { vec![(account, id)] };
    let ids: Vec<_> = selected.iter().map(|(_, id)| *id).collect();
    let numbers: Vec<_> = selected.iter().map(|(a, _)| a.account_number.clone()).collect();
    let before = ctx.vault.lock().unwrap().clone();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let client = Bank { numbers:numbers.clone(), requests:requests.clone() };
    let message = sync_with_client(&ctx, &access, Some(selected), client).unwrap();
    if !group { assert!(message.contains("Selected test account")); }
    let mut expected = Vec::new();
    for number in numbers { expected.extend([format!("balance:{number}"), format!("transactions:{number}")]); }
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
        v.queue_transfer(ungrouped, crate::business_tests::standing().transfer, false, false).unwrap();
        (group_id, first, same_bank, other_bank, ungrouped, empty)
    };
    let targets = group_targets(&ctx, group_id).unwrap();
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].accounts.as_ref().unwrap().iter().map(|(_, id)| *id).collect::<Vec<_>>(), [first, same_bank]);
    assert_eq!(targets[1].accounts.as_ref().unwrap().iter().map(|(_, id)| *id).collect::<Vec<_>>(), [other_bank]);
    let targets = group_targets(&ctx, 0).unwrap();
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].accounts.as_ref().unwrap().iter().map(|(_, id)| *id).collect::<Vec<_>>(), [ungrouped]);
    assert!(group_targets(&ctx, 99999).is_err());
    assert!(group_targets(&ctx, empty).is_err());
    let before = serde_json::to_vec(&*ctx.vault.lock().unwrap()).unwrap();
    sync_group(&ctx, group_id).unwrap(); // orders outside the group remain untouched
    assert_eq!(serde_json::to_vec(&*ctx.vault.lock().unwrap()).unwrap(), before);
}
