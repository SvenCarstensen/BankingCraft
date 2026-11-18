//! SCT Inst dispatch, durable outcome callback, and status-only continuation.
use super::*;
use banking::{InstantOutcome, InstantPayment};
use chrono::Utc;

impl<'a> Client<'a> {
    /// Called as soon as an authenticated bank outcome crosses the worker pipe,
    /// including failed exchanges. Failure to persist aborts all further queries.
    pub fn observe_instant(&mut self, observer:impl Fn(&InstantPayment)->Result<()>+'a) {
        self.instant_observer=Some(Box::new(observer));
    }
    pub(super) fn receive_instant(&mut self,frame:&Frame)->Result<()> {
        let (iban,reference)=self.instant_context.as_ref().ok_or_else(invalid)?;
        let result=parse_instant(frame,iban,reference.as_deref())?;
        if let Some(observer)=&self.instant_observer {observer(&result)?;}
        self.instant_result=Some(result);
        Ok(())
    }
    pub(super) fn complete_instant(&mut self,account:&Account)->Result<String> {
        for _ in 0..3 {
            let result=self.instant_result.clone().ok_or_else(invalid)?;
            if result.outcome!=InstantOutcome::Pending || !result.can_query() {return outcome_message(&result);}
            self.query_instant_once(account,&result)?;
        }
        outcome_message(self.instant_result.as_ref().ok_or_else(invalid)?)
    }
    /// Queries an existing order; this path cannot construct or resend HKIPZ.
    pub fn instant_status(&mut self,account:&Account,previous:&InstantPayment)->Result<String> {
        self.query_instant_once(account,previous)?;
        self.complete_instant(account)
    }
    fn query_instant_once(&mut self,account:&Account,previous:&InstantPayment)->Result<()> {
        if account.iban!=previous.debtor_iban || !previous.can_query() || !valid_reference(&previous.reference) {
            return Err(invalid());
        }
        let wait=(previous.next_query_at-Utc::now()).to_std().unwrap_or_default();
        if wait>Duration::from_secs(100) {return Err(invalid());}
        let generation=self.ui.session_generation();
        let deadline=Instant::now()+wait;
        if !wait.is_zero() { self.ui.progress("Warte auf den nächsten zulässigen Statusabruf der Echtzeitüberweisung …"); }
        while Instant::now()<deadline {
            if self.ui.cancelled() || self.ui.session_generation()!=generation {return Err(Error::Cancelled);}
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()).min(Duration::from_millis(100)));
        }
        let mut frame=self.account_request("instant_status",account)?;
        frame.put("reference",&previous.reference);
        self.instant_context=Some((account.iban.clone(),Some(previous.reference.clone())));
        self.instant_result=None;
        self.call(frame)?;
        if self.instant_result.is_none() {return Err(invalid());}
        Ok(())
    }
}
fn outcome_message(result:&InstantPayment)->Result<String> {
    if result.outcome==InstantOutcome::Executed {Ok(result.message().into())}
    else {Err(Error::Other(result.message().into()))}
}
fn valid_reference(reference:&str)->bool {
    !reference.trim().is_empty() && reference.chars().count()<=99 && !reference.chars().any(char::is_control)
}
fn parse_instant(frame:&Frame,iban:&str,reference:Option<&str>)->Result<InstantPayment> {
    if text(frame,"iban")?!=iban {return Err(invalid());}
    let id=text(frame,"reference")?;
    if (!id.is_empty() && !valid_reference(id)) || reference.is_some_and(|expected|expected!=id) {return Err(invalid());}
    let outcome=match text(frame,"outcome")? {
        "pending"=>InstantOutcome::Pending,"executed"=>InstantOutcome::Executed,
        "rejected"=>InstantOutcome::Rejected,"unknown"=>InstantOutcome::Unknown,_=>return Err(invalid())
    };
    if id.is_empty() && matches!(outcome,InstantOutcome::Executed|InstantOutcome::Pending) {return Err(invalid());}
    let wait=number(frame,"wait_seconds")?;
    if !(0..=99).contains(&wait) {return Err(invalid());}
    Ok(InstantPayment{debtor_iban:iban.into(),reference:id.into(),outcome,
        next_query_at:Utc::now()+chrono::Duration::seconds(wait)})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn result(outcome:&str,id:&str,wait:&str)->Frame {
        let mut f=Frame::new("unused");f.put("event","instant_result");f.put("iban","DE89370400440532013000");
        f.put("reference",id);f.put("outcome",outcome);f.put("wait_seconds",wait);f
    }
    #[test]
    fn outcomes_require_correlated_reference_and_distinguish_execution() {
        let iban="DE89370400440532013000";
        for (state,ok) in [("executed",true),("pending",false),("rejected",false),("unknown",false)] {
            let p=parse_instant(&result(state,"BANK-ORDER-1","12"),iban,Some("BANK-ORDER-1")).unwrap();
            assert_eq!(outcome_message(&p).is_ok(),ok);
            assert_eq!(p.can_query(),state=="pending"||state=="unknown");
            assert!(p.next_query_at>Utc::now()+chrono::Duration::seconds(10));
        }
        for f in [result("executed","","0"),result("pending","x","100"),result("pending","x","-1"),
                  result("accepted","x","0"),result("pending","bad\nreference","0"),result("pending",&"x".repeat(100),"0")] {
            assert!(parse_instant(&f,iban,None).is_err());
        }
        assert!(parse_instant(&result("executed","wrong","0"),iban,Some("correct")).is_err());
        assert!(parse_instant(&result("executed","x","0"),"other account",None).is_err());
        assert!(!parse_instant(&result("unknown","","0"),iban,None).unwrap().can_query());
    }
    #[cfg(unix)]
    fn worker(ui:&dyn Interaction,state:&str,code:i32,log:std::sync::Arc<std::sync::Mutex<Vec<(String,String)>>>) -> (CardWorker,std::thread::JoinHandle<()>) {
        let mut child=Command::new("/bin/cat").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let input=child.stdin.take().unwrap();let mut output=child.stdout.take().unwrap();
        let (send,receive)=mpsc::sync_channel(8);let state=state.to_owned();
        let handle=std::thread::spawn(move || {
            let Ok(request)=Frame::read(&mut output) else {return};
            log.lock().unwrap().push((request.text("op").unwrap().into(),request.text("reference").unwrap_or("").into()));
            let _=send.send(Ok(result(&state,"BANK-ORDER-1","0")));
            let mut done=Frame::new("done");done.put("event","done");done.put("code",code.to_string());
            for (key,value) in [("stage","transfer"),("bad_pin","0"),("cancelled","0"),("notice",""),("state","synthetic")] {done.put(key,value);}
            let _=send.send(Ok(done));
        });
        (CardWorker{worker:Worker(child),input,receive,_blocker:tempfile::NamedTempFile::new().unwrap(),generation:ui.session_generation()},handle)
    }
    #[cfg(unix)]
    struct NoBank;
    #[cfg(unix)]
    impl Interaction for NoBank {
        fn progress(&self,_:&str) {}
        fn pin(&self,_:&str)->Option<String> {panic!("no bank/PIN in offline tests")}
        fn tan(&self,_:&TanChallenge,_:Option<&[u8]>,_:Option<(&str,&[u8])>)->TanAnswer {panic!("no real payment in offline tests")}
    }
    fn payment()->(Account,SepaTransfer) {
        let account=Account{iban:"DE89370400440532013000".into(),..Default::default()};
        let transfer=SepaTransfer{debtor_iban:account.iban.clone(),amount:Decimal::ONE,instant:true,
            debtor_name:"Sender".into(),debtor_bic:"COBADEFFXXX".into(),creditor_name:"Recipient".into(),
            creditor_iban:"DE02120300000000202051".into(),creditor_bic:"BYLADEM1001".into(),
            purpose:"Offline only".into(),end_to_end_id:"offline-1".into(),execution_date:None};
        (account,transfer)
    }
    #[test]
    #[cfg(unix)]
    fn payment_is_sent_once_and_outcomes_are_persisted_before_status_queries() {
        use std::sync::{Arc,Mutex};
        let ui=NoBank;let log=Arc::new(Mutex::new(Vec::new()));let stored=Arc::new(Mutex::new(Vec::new()));
        let (account,t)=payment();let mut client=local_client(Medium::PinTan,&ui).unwrap();
        client.state.aqbanking.as_mut().unwrap().accounts=vec![(42,account.clone())];
        let mut handles=Vec::new();let mut workers=std::collections::VecDeque::new();
        for state in ["pending","pending","executed"] {let (w,h)=worker(&ui,state,0,log.clone());workers.push_back(w);handles.push(h);}
        client.test_workers=Some(workers);
        let saved=stored.clone();let requests=log.clone();
        client.observe_instant(move |p| {
            let count=saved.lock().unwrap().len();
            assert_eq!(requests.lock().unwrap().len(),count+1,"persist before starting the next query");
            saved.lock().unwrap().push(p.clone());Ok(())
        });
        assert!(client.transfer(&account,&t).unwrap().contains("ausgeführt"));
        assert_eq!(*log.lock().unwrap(),[("transfer".into(),"".into()),("instant_status".into(),"BANK-ORDER-1".into()),("instant_status".into(),"BANK-ORDER-1".into())]);
        assert_eq!(stored.lock().unwrap().len(),3);for h in handles {h.join().unwrap();}
    }
    #[test]
    #[cfg(unix)]
    fn uncertainty_rejection_transport_loss_and_save_failure_never_resubmit() {
        use std::sync::{Arc,Mutex};
        for (state,code,save_fails) in [("unknown",0,false),("rejected",0,false),("pending",-1,false),("pending",0,true)] {
            let ui=NoBank;let log=Arc::new(Mutex::new(Vec::new()));let saved=std::cell::RefCell::new(None);
            let (account,t)=payment();let mut client=local_client(Medium::PinTan,&ui).unwrap();
            client.state.aqbanking.as_mut().unwrap().accounts=vec![(42,account.clone())];
            let (w,h)=worker(&ui,state,code,log.clone());client.test_workers=Some([w].into());
            client.observe_instant(|p| { *saved.borrow_mut()=Some(p.clone()); if save_fails {Err(Error::Other("disk full".into()))} else {Ok(())} });
            assert!(client.transfer(&account,&t).is_err());assert_eq!(log.lock().unwrap().len(),1);
            assert_eq!(saved.borrow().as_ref().unwrap().reference,"BANK-ORDER-1");h.join().unwrap();
        }
    }
    #[test]
    #[cfg(unix)]
    fn polling_is_bounded_and_resume_queries_only_the_original_reference() {
        use std::sync::{Arc,Mutex};
        let ui=NoBank;let log=Arc::new(Mutex::new(Vec::new()));let (account,t)=payment();
        let mut client=local_client(Medium::PinTan,&ui).unwrap();client.state.aqbanking.as_mut().unwrap().accounts=vec![(42,account.clone())];
        let mut handles=Vec::new();let mut workers=std::collections::VecDeque::new();
        for _ in 0..4 {let (w,h)=worker(&ui,"pending",0,log.clone());workers.push_back(w);handles.push(h);}
        client.test_workers=Some(workers);assert!(client.transfer(&account,&t).is_err());
        assert_eq!(log.lock().unwrap().iter().filter(|(op,_)|op=="transfer").count(),1);assert_eq!(log.lock().unwrap().len(),4);
        let previous=client.instant_result.clone().unwrap();drop(client);for h in handles {h.join().unwrap();}
        let mut restarted=local_client(Medium::PinTan,&ui).unwrap();restarted.state.aqbanking.as_mut().unwrap().accounts=vec![(42,account.clone())];
        let (w,h)=worker(&ui,"executed",0,log.clone());restarted.test_workers=Some([w].into());
        assert!(restarted.instant_status(&account,&previous).is_ok());h.join().unwrap();
        assert_eq!(log.lock().unwrap().last().unwrap(),&("instant_status".into(),"BANK-ORDER-1".into()));
    }
    #[test]
    fn cancellation_and_invalid_dates_stop_before_any_worker() {
        struct Cancelled;
        impl Interaction for Cancelled {
            fn progress(&self,_:&str) {}
            fn pin(&self,_:&str)->Option<String> {panic!()}
            fn tan(&self,_:&TanChallenge,_:Option<&[u8]>,_:Option<(&str,&[u8])>)->TanAnswer {panic!()}
            fn cancelled(&self)->bool {true}
        }
        let (account,mut t)=payment();let mut client=local_client(Medium::PinTan,&Cancelled).unwrap();
        client.test_workers=Some(Default::default());client.state.aqbanking.as_mut().unwrap().accounts=vec![(42,account.clone())];
        let mut p=parse_instant(&result("pending","BANK-ORDER-1","2"),&account.iban,None).unwrap();
        assert!(matches!(client.instant_status(&account,&p),Err(Error::Cancelled)));
        p.debtor_iban="other".into();assert!(client.instant_status(&account,&p).is_err());
        for date in [None,Some(chrono::Local::now().date_naive())] {
            t.instant=false;t.execution_date=date;assert!(matches!(client.scheduled_transfer(&account,&t),Err(Error::Security(_))));
        }
        t.instant=true;t.execution_date=Some(chrono::Local::now().date_naive()+chrono::Duration::days(5));
        assert!(matches!(client.transfer(&account,&t),Err(Error::Security(_))));
        assert!(matches!(client.scheduled_transfer(&account,&t),Err(Error::Security(_))));
    }

    #[test]
    #[cfg(unix)]
    fn status_query_waits_for_the_banks_minimum_interval() {
        use std::sync::{Arc,Mutex};
        let ui=NoBank;let log=Arc::new(Mutex::new(Vec::new()));let (account,_)=payment();
        let mut client=local_client(Medium::PinTan,&ui).unwrap();client.state.aqbanking.as_mut().unwrap().accounts=vec![(42,account.clone())];
        let (w,h)=worker(&ui,"executed",0,log);client.test_workers=Some([w].into());
        let previous=parse_instant(&result("pending","BANK-ORDER-1","1"),&account.iban,None).unwrap();
        let start=Instant::now();assert!(client.instant_status(&account,&previous).is_ok());
        assert!(start.elapsed()>=Duration::from_millis(990));h.join().unwrap();
    }

}
