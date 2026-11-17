//! One bounded, read-only AqHBCI queue for the selected card accounts.
use super::*;

const MAX_ACCOUNTS:usize=16;

pub struct ReadRequest {
    pub account:Account,
    pub balance:bool,
    pub transactions:bool,
    pub from:NaiveDate,
}
pub(super) struct ReadPlan { id:u32, balance:bool, transactions:bool, from:NaiveDate }
pub(super) struct ReadResult {
    id:u32,
    from:NaiveDate,
    balance:Option<Result<Option<Balance>>>,
    transactions:Option<Result<TransactionReport>>,
}

impl Client<'_> {
    /// Plan only: the first selected read submits a batch, later reads consume
    /// its results once. A later chunk failing cannot discard applied chunks.
    pub fn prepare_reads(&mut self,requests:Vec<ReadRequest>) -> Result<bool> {
        self.planned_reads.clear(); self.prefetched_reads.clear(); self.read_generation=None;
        if matches!(self.medium,Medium::PinTan) {return Ok(false);}
        let mut plans=Vec::<ReadPlan>::new();
        for request in requests {
            if !(request.balance || request.transactions) {return Err(invalid());}
            let (id,_)=self.known_account(&request.account)?;
            if plans.iter().any(|p|p.id==id) {return Err(invalid());}
            plans.push(ReadPlan {id,balance:request.balance,transactions:request.transactions,from:request.from});
        }
        self.read_generation=Some(self.ui.session_generation());
        self.planned_reads=plans;
        Ok(!self.planned_reads.is_empty())
    }
    fn prefetch_for(&mut self,id:u32) -> Result<()> {
        if self.ui.cancelled() || self.read_generation.is_some_and(|g|g!=self.ui.session_generation()) {return Err(Error::Cancelled);}
        if !self.planned_reads.iter().any(|p|p.id==id) {return Ok(());}
        // Calls normally follow the plan. Keep arbitrary account requests scoped
        // to the chunk containing that account, without fetching unselected ones.
        let start=self.planned_reads.iter().position(|p|p.id==id).ok_or_else(invalid)?;
        let end=(start+MAX_ACCOUNTS).min(self.planned_reads.len());
        let plans=self.planned_reads.drain(start..end).collect::<Vec<_>>();
        let mut request=Frame::new("read_batch"); request.put("count",plans.len().to_string());
        for (i,plan) in plans.iter().enumerate() {
            request.put(&format!("account{i}"),plan.id.to_string());
            request.put(&format!("balance{i}"),if plan.balance {"1"} else {"0"});
            request.put(&format!("transactions{i}"),if plan.transactions {"1"} else {"0"});
            date_field(&mut request,&format!("from{i}"),Some(plan.from));
        }
        let frames=self.call(request)?;
        self.prefetched_reads.extend(parse_read_results(frames,&plans)?);
        Ok(())
    }
    pub(super) fn cached_balance(&mut self,account:&Account) -> Result<Option<Result<Option<Balance>>>> {
        let (id,_)=self.known_account(account)?;
        self.prefetch_for(id)?;
        Ok(self.prefetched_reads.iter_mut().find(|r|r.id==id).and_then(|r|r.balance.take()))
    }
    pub(super) fn cached_transactions(&mut self,account:&Account,from:NaiveDate,to:Option<NaiveDate>) -> Result<Option<Result<TransactionReport>>> {
        let (id,_)=self.known_account(account)?;
        if to.is_none() {self.prefetch_for(id)?;}
        Ok(self.prefetched_reads.iter_mut().find(|r|r.id==id && r.from==from && to.is_none()).and_then(|r|r.transactions.take()))
    }
}

fn read_error(frame:&Frame,kind:&str,code:i64) -> Result<Error> {
    if code==-68 {return Ok(Error::NotSupported("AqBanking unterstützt diesen Abruf mit den aktuellen Kontodaten nicht.".into()));}
    let mut error=Frame::new("done"); error.put("stage",kind); error.put("notice",frame.bytes("notice"));
    error.put("bad_pin",frame.bytes("bad_pin"));
    Ok(match native_error(&error,code)? {Error::Bank{code,text}=>Error::Access{code,text},other=>other})
}
fn parse_read_results(frames:Vec<Frame>,plans:&[ReadPlan]) -> Result<Vec<ReadResult>> {
    let mut groups=Vec::<(Frame,Vec<Frame>)>::new();
    for frame in frames {
        match text(&frame,"event")? {
            "account"=>{}, // updated capabilities are handled by Client::call
            "read_result"=>groups.push((frame,Vec::new())),
            "balance"|"transaction"=>groups.last_mut().ok_or_else(invalid)?.1.push(frame),
            _=>return Err(invalid()),
        }
    }
    if groups.len()!=plans.len() {return Err(invalid());}
    let mut results=Vec::<ReadResult>::new();
    for (header,data) in groups {
        let id=u32::try_from(number(&header,"account")?).map_err(|_|invalid())?;
        let plan=plans.iter().find(|p|p.id==id).ok_or_else(invalid)?;
        if results.iter().any(|r|r.id==id) {return Err(invalid());}
        let balance_code=number(&header,"balance_code")?;
        let transactions_code=number(&header,"transactions_code")?;
        let balance=if !plan.balance {None} else {Some(if balance_code==0 {Ok(parse_balance(&data)?)} else {Err(read_error(&header,"balance",balance_code)?)})};
        let transactions=if !plan.transactions {None} else {Some(if transactions_code==0 {Ok(parse_transaction_report(&data)?)} else {Err(read_error(&header,"transactions",transactions_code)?)})};
        results.push(ReadResult {id,from:plan.from,balance,transactions});
    }
    Ok(results)
}

#[cfg(all(test,unix))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64,Ordering};
    #[derive(Default)]
    struct Ui(AtomicU64);
    impl Interaction for Ui {
        fn session_generation(&self)->u64 {self.0.load(Ordering::Acquire)}
        fn pin(&self,_:&str)->Option<String> {panic!("offline fixture")}
        fn tan(&self,_:&TanChallenge,_:Option<&[u8]>,_:Option<(&str,&[u8])>)->TanAnswer {panic!("offline fixture")}
        fn progress(&self,_:&str) {}
    }
    fn header(id:u32,balance:i64,transactions:i64)->Frame {
        let mut f=Frame::new("read_result"); f.put("event","read_result");f.put("account",id.to_string());
        f.put("balance_code",balance.to_string());f.put("transactions_code",transactions.to_string());
        f.put("bad_pin","0");f.put("notice","Synthetic bank result");f
    }
    fn balance(id:u32)->Frame {
        let mut f=Frame::new("balance");for (k,v) in [("event","balance"),("type","2"),("date","20261117"),("currency","EUR")] {f.put(k,v);}
        f.put("amount",id.to_string());f
    }
    fn worker(ui:&dyn Interaction,fail_second:bool)->(CardWorker,std::thread::JoinHandle<Vec<usize>>) {
        let mut child=Command::new("/bin/cat").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let input=child.stdin.take().unwrap();let mut output=child.stdout.take().unwrap();
        let (send,receive)=mpsc::sync_channel(128);
        let thread=std::thread::spawn(move || {
            let mut counts=Vec::new();
            for batch in 0..2 {
                let request=Frame::read(&mut output).unwrap();assert_eq!(request.text("op").unwrap(),"read_batch");
                assert!(request.bytes("state").is_empty() && request.bytes("user").is_empty());
                let count=request.number("count").unwrap() as usize;counts.push(count);
                if !(fail_second && batch==1) {
                    // Responses need not have the same order as requests.
                    for i in (0..count).rev() {
                        let id=request.number(&format!("account{i}")).unwrap() as u32;
                        assert_eq!(request.text(&format!("balance{i}")).unwrap(),"1");
                        assert_eq!(request.text(&format!("transactions{i}")).unwrap(),"1");
                        assert_eq!(request.text(&format!("from{i}")).unwrap(),format!("202611{id:02}"));
                        send.send(Ok(header(id,0,0))).unwrap();send.send(Ok(balance(id))).unwrap();
                        let mut tx=balance(id);tx.put("event","transaction");tx.put("type","128");tx.put("status","1");
                        tx.put("amount",format!("{id}/100"));tx.put("purpose",format!("Account {id}"));
                        send.send(Ok(tx)).unwrap();
                    }
                }
                let failed=fail_second && batch==1;
                let mut done=Frame::new("done");for (k,v) in [("event","done"),("stage","read_batch"),("bad_pin","0"),("cancelled","0"),("state","synthetic-state"),("notice","Synthetic failure")] {done.put(k,v);}
                done.put("code",if failed {"-1"} else {"0"});done.put("session",if failed {"0"} else {"1"});
                send.send(Ok(done)).unwrap();
            }
            counts
        });
        (CardWorker {worker:Worker(child),input,receive,_blocker:tempfile::NamedTempFile::new().unwrap(),generation:ui.session_generation()},thread)
    }
    #[test]
    fn eighteen_accounts_use_two_read_batches_and_keep_dates_and_balances_separate() {
        for fail_second in [false,true] {
            let ui=Ui::default();let mut client=local_client(Medium::Rdh {reader:String::new(),record:1},&ui).unwrap();
            let accounts=(1..=18).map(|id|Account {account_number:id.to_string(),blz:"12345678".into(),..Default::default()}).collect::<Vec<_>>();
            client.state.aqbanking.as_mut().unwrap().accounts=accounts.iter().enumerate().map(|(i,a)|(i as u32+1,a.clone())).collect();
            assert!(client.prepare_reads(accounts.iter().enumerate().map(|(i,a)|ReadRequest {account:a.clone(),balance:true,transactions:true,from:NaiveDate::from_ymd_opt(2026,11,i as u32+1).unwrap()}).collect()).unwrap());
            let (worker,thread)=worker(&ui,fail_second);client.session=Some(worker);
            for (i,account) in accounts.iter().enumerate() {
                if fail_second && i==16 {assert!(matches!(client.balance(account),Err(Error::Access{..})));break;}
                let amount=Decimal::from(i+1);
                assert_eq!(client.balance(account).unwrap().unwrap().booked,amount);
                let report=client.transaction_report(account,NaiveDate::from_ymd_opt(2026,11,i as u32+1).unwrap(),None).unwrap();
                assert_eq!(report.balance.unwrap().booked,amount);assert_eq!(report.transactions.len(),1);
                assert_eq!(report.transactions[0].amount,Decimal::new(i as i64+1,2));
                assert_eq!(report.transactions[0].purpose,format!("Account {}",i+1));
            }
            assert_eq!(thread.join().unwrap(),[16,2]); // 36 single jobs -> two bounded queues, without retry
            if !fail_second {
                ui.0.store(1,Ordering::Release);
                assert!(matches!(client.cached_balance(&accounts[0]),Err(Error::Cancelled)));
            }
        }
    }
    #[test]
    fn batch_results_reject_cross_account_missing_and_duplicate_data_and_preserve_errors() {
        let from=NaiveDate::from_ymd_opt(2026,11,1).unwrap();
        let plans=[ReadPlan {id:42,balance:true,transactions:true,from},ReadPlan {id:43,balance:false,transactions:true,from}];
        for frames in [vec![],vec![balance(42),header(42,0,0),header(43,0,0)],vec![header(42,0,0),header(42,0,0)],vec![header(42,0,0),header(99,0,0)]] {
            assert!(parse_read_results(frames,&plans).is_err());
        }
        let mut results=parse_read_results(vec![header(43,0,-68),header(42,-1,0)],&plans).unwrap();
        assert!(results[0].balance.is_none());assert!(matches!(results[0].transactions.take(),Some(Err(Error::NotSupported(_)))));
        assert!(matches!(results[1].balance.take(),Some(Err(Error::Access{..}))));assert!(results[1].transactions.take().unwrap().is_ok());
        let ui=Ui::default();let mut client=local_client(Medium::PinTan,&ui).unwrap();
        assert!(!client.prepare_reads(Vec::new()).unwrap());
    }
}
