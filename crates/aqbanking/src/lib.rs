//! AqBanking runs in a disposable native process, with an in-memory config
//! manager. Only opaque snapshots cross back into the encrypted vault.
mod reads;
mod instant;
mod reader_prompt;
pub use reads::ReadRequest;
mod wire;
mod readers;
mod product;
pub use readers::{list_readers, ReaderInfo};
use wire::Frame;
use chrono::NaiveDate;
use banking::{Account, Balance, Config, Error, Interaction, Interval, Result, SepaTransfer, StandingOrder, State, TanAnswer, TanChallenge, TanMethod, Transaction, TransactionReport};
use banking::AqBankingState;
use rust_decimal::Decimal;
use sha2::{Digest,Sha256};
use std::{io, path::PathBuf, process::{Child,ChildStdin,Command,Stdio}, sync::mpsc, time::{Duration,Instant}};
use zeroize::{Zeroize,Zeroizing};

#[derive(Clone)]
pub enum Medium { PinTan, Ddv { reader:String, record:u8 }, Rdh { reader:String, record:u8 } }
impl Medium {
    fn name(&self) -> &str { match self { Self::PinTan=>"pintan", Self::Ddv{..}=>"ddvcard", Self::Rdh{..}=>"zkacard" } }
    fn reader(&self) -> &str { match self { Self::PinTan=>"", Self::Ddv{reader,..}|Self::Rdh{reader,..}=>reader } }
    fn record(&self) -> u8 { match self { Self::PinTan=>1, Self::Ddv{record,..}|Self::Rdh{record,..}=>*record } }
}
fn failure(_: io::Error) -> Error { Error::Transport("Banking-Prozess beendet oder ungültige Antwort. Status eines gesendeten Auftrags vor erneutem Senden prüfen.".into()) }
fn invalid() -> Error { Error::Security("Ungültige Daten vom Banking-Prozess".into()) }
fn text<'a>(frame:&'a Frame,key:&str) -> Result<&'a str> { frame.text(key).map_err(|_|invalid()) }
fn number(frame:&Frame,key:&str) -> Result<i64> { frame.number(key).map_err(|_|invalid()) }
fn native_error(frame:&Frame,code:i64) -> Result<Error> {
    let stage=text(frame,"stage")?;
    let (label,access)=match stage {
        "initialize"=>("Verbindungseinrichtung",true),
        "select_card_context"=>("Bankzugang auf der Chipkarte auswählen",true),
        "getkeys"=>("Bankschlüssel abrufen",true),
        "getbankinfo"=>("Bankparameter abrufen",true),
        "getsysid"=>("Systemkennung synchronisieren",true),
        "select_tan"|"getitanmodes"=>("TAN-Verfahren einrichten",true),
        "select_tan_medium"=>("TAN-Medium einrichten",true),
        "configure_accounts"=>("Umsatzabruf einrichten",true),
        "getaccounts"|"accounts"=>("Konten abrufen",true),
        "getaccsepa"=>("SEPA-Kontodaten abrufen",true),
        "balance"=>("Saldo abrufen",false),
        "transactions"=>("Umsätze abrufen",false),
        "read_batch"=>("Salden und Umsätze abrufen",false),
        "instant_status"=>("Status der Echtzeitüberweisung abrufen",false),
        "transfer"|"scheduled"=>("Überweisung freigeben",false),
        "create_standing"|"delete_standing"=>("Dauerauftrag freigeben",false),
        _=>("Bankauftrag",false),
    };
    let notice=banking::display::strip_html(text(frame,"notice")?);
    let notice=if notice.trim().is_empty() {"Keine nähere Fehlermeldung verfügbar."} else {notice.trim()};
    let message=format!("AqBanking – {label}:\n{notice}\n(AqBanking-Status {code})");
    let bank_code=if number(frame,"bad_pin")?!=0 {"9930".into()} else {code.to_string()};
    // Only a local account-capability rejection is an unsupported operation.
    // -68 can also originate in setup/cryptography; preserve those failures.
    if code == -68 && !access && bank_code != "9930"
        && frame.text("unsupported_account_job").unwrap_or("0") == "1" {
        return Ok(Error::NotSupported(message));
    }
    Ok(if access {Error::Access {code:bank_code,text:message}} else {Error::Bank {code:bank_code,text:message}})
}
fn parse_accounts(frames:&[Frame]) -> Result<Vec<(u32,Account)>> {
    let mut accounts=Vec::new();
    for frame in frames {
        if text(frame,"event")?!="account" { continue; }
        let mut account=Account::default();
        macro_rules! field {($name:ident,$key:expr)=>{account.$name=text(frame,$key)?.into();}}
        field!(iban,"iban");field!(bic,"bic");field!(account_number,"number");field!(sub_account,"sub");field!(blz,"blz");
        field!(owner,"owner");field!(product,"name");field!(currency,"currency");
        account.kind=match number(frame,"kind")? {2=>50,4|7=>10,5=>30,8=>40,_=>1};
        account.allowed=text(frame,"allowed")?.lines().map(str::to_owned).collect();
        if account.allowed.is_empty() { account.allowed.push("AQBANKING_NO_SUPPORTED_JOBS".into()); }
        let id=u32::try_from(number(frame,"id")?).map_err(|_|invalid())?;
        if id==0 || accounts.iter().any(|(known,_)| *known==id) { return Err(invalid()); }
        accounts.push((id,account));
    }
    Ok(accounts)
}
fn date(frame:&Frame,key:&str) -> Result<Option<NaiveDate>> {
    let s=text(frame,key)?; if s.is_empty() { return Ok(None); }
    NaiveDate::parse_from_str(s,"%Y%m%d").or_else(|_|NaiveDate::parse_from_str(s,"%Y-%m-%d")).map(Some).map_err(|_|invalid())
}
fn amount(frame:&Frame) -> Result<Decimal> {
    let s=text(frame,"amount")?;
    let (n,d)=s.split_once('/').unwrap_or((s,"1"));
    let n:Decimal=n.parse().map_err(|_|invalid())?; let d:Decimal=d.parse().map_err(|_|invalid())?;
    let value=n.checked_div(d).ok_or_else(invalid)?;
    if value.checked_mul(d)!=Some(n) { return Err(invalid()); }
    Ok(value.normalize())
}
fn date_field(frame:&mut Frame,key:&str,value:Option<NaiveDate>) { if let Some(d)=value { frame.put(key,d.format("%Y%m%d").to_string()); } }

fn helper_path() -> Result<PathBuf> {
    let executable=std::env::current_exe().map_err(failure)?;
    let name=if cfg!(windows) {"bankingcraft-aqbanking.exe"} else {"bankingcraft-aqbanking"};
    let packaged=executable.parent().ok_or_else(invalid)?.join("native/bin").join(name);
    if packaged.is_file() { return Ok(packaged); }
    let adjacent=executable.with_file_name(name);
    if adjacent.is_file() { return Ok(adjacent); }
    if cfg!(debug_assertions) {
        if let Some(path)=std::env::var_os("BANKINGCRAFT_AQBANKING_HELPER") {
            let path=PathBuf::from(path); if path.is_absolute() && path.is_file() { return Ok(path); }
        }
        let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        if let Ok(path)=std::fs::read_to_string(root.join(".native/helper-path")) {
            let path=PathBuf::from(path.trim()); if path.is_absolute() && path.is_file() { return Ok(path); }
        }
    }
    Err(Error::Other("AqBanking/Libchipcard fehlen. Bitte das vollständige BankingCraft-Paket installieren; Entwicklungsbuilds benötigen scripts/build-native.py.".into()))
}
struct Worker(Child);
impl Drop for Worker { fn drop(&mut self) { let _=self.0.kill(); let _=self.0.wait(); } }

// One native card session belongs to one Client/access and one UI generation.
// No PIN bytes are cached here; Libchipcard retains the authenticated card handle.
struct CardWorker {
    worker:Worker,
    input:ChildStdin,
    receive:mpsc::Receiver<io::Result<Frame>>,
    _blocker:tempfile::NamedTempFile,
    generation:u64,
}
impl CardWorker {
    fn close(mut self) {
        let _=Frame::new("close").write(&mut self.input);
        drop(self.input);
        // Normal teardown lets Libchipcard unmount; never wait indefinitely on
        // a stuck driver. Drop still kills/reaps workers on errors/cancellation.
        for _ in 0..50 {
            if self.worker.0.try_wait().ok().flatten().is_some() { return; }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
fn read_operation(op:&str) -> bool { matches!(op,"open"|"accounts"|"balance"|"transactions"|"read_batch") }

type InstantObserver<'a> = Box<dyn Fn(&banking::InstantPayment)->Result<()>+'a>;

pub struct Client<'a> {
    config:Config,
    pub state:State,
    medium:Medium,
    ui:&'a dyn Interaction,
    session:Option<CardWorker>,
    planned_reads:Vec<reads::ReadPlan>,
    read_generation:Option<u64>,
    prefetched_reads:Vec<reads::ReadResult>,
    #[cfg(test)]
    test_workers:Option<std::collections::VecDeque<CardWorker>>,
    payment_notice:String,
    instant_context:Option<(String,Option<String>)>,
    instant_result:Option<banking::InstantPayment>,
    instant_observer:Option<InstantObserver<'a>>,
}
impl<'a> Client<'a> {
    pub fn new(config:Config,mut state:State,medium:Medium,ui:&'a dyn Interaction) -> Result<Self> {
        if matches!(medium,Medium::PinTan) { banking::validation::validate_https_url(&config.url)?; }
        else {
            let value=if config.url.contains("://") {config.url.clone()} else {format!("hbci://{}",config.url)};
            let url=url::Url::parse(&value).map_err(|_|invalid())?;
            if !matches!(url.scheme(),"hbci"|"https") || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
                return Err(Error::Security("Ungültige HBCI-Bankadresse".into()));
            }
        }
        let mut hash=Sha256::new();
        for value in [&config.blz,&config.user_id,&config.customer_id,&config.url,medium.name()] { hash.update((value.len() as u64).to_be_bytes()); hash.update(value.as_bytes()); }
        hash.update([medium.record()]); let binding=format!("{:x}",hash.finalize());
        match state.aqbanking.as_ref() {
            Some(native) if native.version!=1 => return Err(Error::Security("Unbekannte AqBanking-Tresorversion; bitte BankingCraft aktualisieren.".into())),
            Some(native) if native.binding!=binding => state.aqbanking=None,
            _=>{},
        }
        if state.aqbanking.is_none() {
            state.aqbanking=Some(AqBankingState {version:1,binding,snapshot:Vec::new(),accounts:Vec::new()});
        }
        Ok(Self {config,state,medium,ui,session:None,read_generation:None,planned_reads:Vec::new(),prefetched_reads:Vec::new(),#[cfg(test)] test_workers:None, payment_notice:String::new(),instant_context:None,instant_result:None,instant_observer:None})
    }
    pub fn finish(mut self) -> State {
        if let Some(worker)=self.session.take() { worker.close(); }
        self.state
    }
    fn configure_request(&self,request:&mut Frame,registration:&str) {
        request.put("state",&self.state.aqbanking.as_ref().unwrap().snapshot);
        request.put("blz",&self.config.blz); request.put("user",&self.config.user_id);
        request.put("customer",if self.config.customer_id.is_empty() {&self.config.user_id} else {&self.config.customer_id});
        request.put("url",&self.config.url);
        // Postbank uses push approval. Select only a BestSign method actually
        // advertised for this user; never invent a security-function number.
        if matches!(self.medium,Medium::PinTan)
            && url::Url::parse(&self.config.url).ok().and_then(|u|u.host_str().map(str::to_owned)).as_deref()==Some("hbci.postbank.de") {
            request.put("preferred_tan","bestsign");
            request.put("skip_matching_vop","1");
        }
        product::configure(request,registration);
        request.put("medium",self.medium.name()); request.put("record",self.medium.record().to_string());
    }
    fn spawn_worker(&self,request:&mut Frame,session:bool) -> Result<CardWorker> {
        let generation=self.ui.session_generation();
        let helper=helper_path()?;
        let blocker=tempfile::NamedTempFile::new().map_err(failure)?;
        request.put("version","1"); request.put("data_path",blocker.path().to_str().ok_or_else(invalid)?);
        if session { request.put("card_session","1"); }
        self.configure_request(request,&product::registration()?);
        let mut command=Command::new(helper);
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        command.env_remove("AQBANKING_HOME").env_remove("AQHBCI_LOGLEVEL").env_remove("GWEN_LOGLEVEL");
        command.env_remove("LC_LOGLEVEL").env_remove("LD_PRELOAD").env_remove("DYLD_INSERT_LIBRARIES");
        command.env("BANKINGCRAFT_CARD_READER",self.medium.reader());
        #[cfg(windows)] { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
        let mut worker=Worker(command.spawn().map_err(failure)?);
        let input=worker.0.stdin.take().ok_or_else(invalid)?;
        let mut output=worker.0.stdout.take().ok_or_else(invalid)?;
        let (send,receive)=mpsc::sync_channel(8);
        std::thread::spawn(move || loop {
            let frame=Frame::read(&mut output); let failed=frame.is_err();
            if send.send(frame).is_err() || failed { break; }
        });
        Ok(CardWorker {worker,input,receive,_blocker:blocker,generation})
    }
    fn next_worker(&mut self,request:&mut Frame,session:bool)->Result<CardWorker> {
        #[cfg(test)]
        if let Some(workers)=&mut self.test_workers { return workers.pop_front().ok_or_else(invalid); }
        self.spawn_worker(request,session)
    }
    fn call(&mut self,request:Frame) -> Result<Vec<Frame>> {
        self.call_with_reader_timeout(request,reader_prompt::PIN_TIMEOUT)
    }
    fn call_with_reader_timeout(&mut self,mut request:Frame,pin_timeout:Duration) -> Result<Vec<Frame>> {
        if self.ui.cancelled() { self.session.take(); return Err(Error::Cancelled); }
        let use_session=!matches!(self.medium,Medium::PinTan) && read_operation(text(&request,"op")?);
        if !use_session {
            self.planned_reads.clear(); self.prefetched_reads.clear(); self.read_generation=None;
            if let Some(worker)=self.session.take() { worker.close(); }
        }
        let mut worker=if let Some(worker)=self.session.take() {
            if worker.generation!=self.ui.session_generation() { return Err(Error::Cancelled); }
            worker
        } else { self.next_worker(&mut request,use_session)? };
        request.write(&mut worker.input).map_err(failure)?; drop(request);
        let mut reader=reader_prompt::ReaderPrompt::new(self.ui,pin_timeout);
        let mut frames=Vec::new(); let mut bytes=0usize; let mut idle=Instant::now(); let mut start=Instant::now(); let mut events=0usize;
        loop {
            if self.ui.cancelled() || self.ui.session_generation() != worker.generation { return Err(Error::Cancelled); }
            reader.tick(Instant::now())?;
            if idle.elapsed()>Duration::from_secs(90) || start.elapsed()>Duration::from_secs(15*60) {
                return Err(Error::Transport("AqBanking-Zeitlimit erreicht. Bankstatus eines gesendeten Auftrags prüfen.".into()));
            }
            let frame=match worker.receive.recv_timeout(Duration::from_millis(100)) {
                Ok(frame)=>frame.map_err(failure)?,
                Err(mpsc::RecvTimeoutError::Timeout)=>continue,
                Err(_)=>return Err(invalid()),
            };
            idle=Instant::now(); events+=1;
            if events>100_000 { return Err(invalid()); }
            match text(&frame,"event")? {
                "done"=>{
                    // Persist bank state even for an error/cancellation. No retry.
                    let snapshot=&mut self.state.aqbanking.as_mut().unwrap().snapshot;
                    snapshot.zeroize(); *snapshot=frame.bytes("state").to_vec();
                    // The worker exports current account capabilities before
                    // dispatch, including on an unsupported selected request.
                    if frames.iter().any(|f:&Frame| f.bytes("event")==b"account") {
                        self.state.aqbanking.as_mut().unwrap().accounts=parse_accounts(&frames)?;
                    }
                    if number(&frame,"bad_pin")?!=0 { self.ui.pin_rejected(); }
                    if number(&frame,"cancelled")?!=0 { return Err(Error::Cancelled); }
                    let code=number(&frame,"code")?;
                    self.payment_notice=banking::display::strip_html(text(&frame,"notice")?);
                    let result=if code!=0 { Err(native_error(&frame,code)?) } else { Ok(frames) };
                    if use_session && frame.text("session").unwrap_or("0")=="1"
                        && (result.is_ok() || matches!(&result,Err(Error::NotSupported(_)))) {
                        self.session=Some(worker);
                    }
                    // A failed card read must stop the account loop, rather than
                    // reopen the card and request the PIN again for every account.
                    return if use_session {
                        result.map_err(|e| match e { Error::Bank{code,text}=>Error::Access{code,text},other=>other })
                    } else { result };
                },
                "instant_result"=>self.receive_instant(&frame)?,
                "progress"=>self.ui.progress(&banking::display::strip_html(text(&frame,"text")?)),
                "show"=>reader.show(&frame,Instant::now())?,
                "hide"=>reader.hide(&frame)?,
                "bad_pin"=>self.ui.pin_rejected(),
                "password"|"message"|"vop"|"choice"=>{
                    if reader.active() {return Err(invalid());} // cannot block the reader deadline on another prompt
                    let automatic=frame.bytes("event")==b"password" && frame.bytes("automatic")==b"1";
                    let reply=if automatic { self.wait_for_approval(&frame,start+Duration::from_secs(15*60),worker.generation)? }
                              else { self.answer(&frame)? };
                    reply.write(&mut worker.input).map_err(failure)?;
                    idle=Instant::now();
                    if !automatic { start=idle; }
                },
                "account"|"balance"|"transaction"|"card"|"reader"|"tan"|"read_result"=>{
                    // Frames are bounded individually and the batch is bounded by
                    // data size, not just the number of transactions.
                    bytes+=frame.size();
                    if bytes>64*1024*1024 { return Err(invalid()); }
                    frames.push(frame);
                },
                _=>return Err(invalid()),
            }
        }
    }
    fn wait_for_approval(&self,frame:&Frame,deadline:Instant,generation:u64) -> Result<Frame> {
        let wait=number(frame,"wait_seconds")?;
        let attempt=number(frame,"attempt")?;
        let limit=number(frame,"limit")?;
        if number(frame,"flags")?&0x20==0 || number(frame,"method")?!=7 || frame.bytes("automatic")!=b"1"
            || !(0..=999).contains(&wait) || !(1..=999).contains(&limit) || !(1..=limit).contains(&attempt) { return Err(invalid()); }
        let title=banking::display::strip_html(text(frame,"title")?);
        let message=banking::display::strip_html(text(frame,"text")?);
        let next=Instant::now()+Duration::from_secs(wait as u64);
        let mut previous=None;
        loop {
            if self.ui.cancelled() || self.ui.session_generation()!=generation { return Err(Error::Cancelled); }
            if Instant::now()>=deadline { return Err(Error::Transport("Zeitlimit der Freigabeprüfung erreicht. Bitte Bankstatus vor erneutem Senden prüfen.".into())); }
            let remaining=next.saturating_duration_since(Instant::now());
            let seconds=remaining.as_secs()+u64::from(remaining.subsec_nanos()!=0);
            if previous!=Some(seconds) {
                self.ui.approval_pending(&title,&message,seconds as u32,attempt as u32,limit as u32);
                previous=Some(seconds);
            }
            if remaining.is_zero() { break; }
            std::thread::sleep(remaining.min(Duration::from_millis(100)));
        }
        // Empty acknowledgement requests HKTAN S; it never authorizes success.
        let mut reply=Frame::new("answer"); reply.put("value",""); Ok(reply)
    }
    fn answer(&self,frame:&Frame) -> Result<Frame> {
        let mut reply=Frame::new("answer");
        let title=banking::display::strip_html(text(frame,"title")?);
        let message=banking::display::strip_html(text(frame,"text")?);
        let value=match text(frame,"event")? {
            "choice"=>{
                let options=frame.entries().filter_map(|(key,_)| key.parse::<u32>().ok().map(|_|key))
                    .map(|key|Ok((key.to_owned(),text(frame,key)?.to_owned()))).collect::<Result<Vec<_>>>()?;
                self.ui.select(&title,&message,&options)
            },
            "vop"=>self.ui.confirm("Empfängerprüfung der Bank",&message).then(||"1".to_string()),
            "message"=>{
                let options=(1..=3).filter_map(|i| {
                    let key=format!("button{i}"); let label=text(frame,&key).ok()?;
                    (!label.is_empty()).then(||(i.to_string(),label.replace('&',"")))
                }).collect::<Vec<_>>();
                self.ui.select(&title,&message,&options)
            },
            "password"=>{
                let flags=number(frame,"flags")?;
                if flags&0x20!=0 { self.answer_tan(frame,&title,&message)? }
                else if flags&2!=0 { self.ui.text(&title,&message) }
                else {
                    if flags&8!=0 { self.ui.pin_rejected(); }
                    self.ui.secret(&title,&message,matches!(self.medium,Medium::PinTan) && flags&(8|128)==0)
                }
            },
            _=>return Err(invalid()),
        };
        if let Some(value)=value {
            let value=Zeroizing::new(value);
            if value.as_bytes().contains(&0) { return Err(invalid()); }
            reply.put("value",value.as_bytes());
        } else { reply.put("cancel","1"); }
        Ok(reply)
    }
    fn answer_tan(&self,frame:&Frame,title:&str,message:&str) -> Result<Option<String>> {
        let method=frame.number("method").unwrap_or(1);
        let rendered=if matches!(method,3|4) && !frame.bytes("challenge").is_empty() {
            let hex=text(frame,"challenge")?;
            if hex.len()>494 || hex.len()%2!=0 || !hex.is_ascii() {return Err(invalid());}
            let bytes=(0..hex.len()).step_by(2).map(|i|u8::from_str_radix(&hex[i..i+2],16).map_err(|_|invalid())).collect::<Result<Vec<_>>>()?;
            if bytes.is_empty() || usize::from(bytes[0])+1!=bytes.len() {return Err(invalid());}
            Some(bytes)
        } else {None};
        let image=if !frame.bytes("image").is_empty() {Some((text(frame,"mime")?,frame.bytes("image")))} else {None};
        if image.is_some_and(|(_,image)|image.len()>1024*1024) {return Err(invalid());}
        let challenge=TanChallenge {text:message.into(),decoupled:method==7,
            method:TanMethod {name:title.into(),technical_id:match method {3|4=>"HHD1.4OPT",5=>"QR",6=>"photoTAN",_=>""}.into(),
                decoupled:method==7,max_tan_length:u32::try_from(number(frame,"max")?).map_err(|_|invalid())?}};
        match self.ui.tan(&challenge,rendered.as_deref(),image) {
            TanAnswer::Tan(t) if !challenge.decoupled && !t.is_empty()=>Ok(Some(t)),
            TanAnswer::Confirmed if challenge.decoupled=>Ok(Some(String::new())),
            TanAnswer::Cancel=>Ok(None),
            _=>Err(invalid()),
        }
    }
    pub fn open(&mut self) -> Result<()> { self.call(Frame::new("open")).map(|_|()) }
    pub fn reset_tan(&mut self) -> Result<()> { self.call(Frame::new("reset_tan")).map(|_|()) }
    pub fn accounts(&mut self) -> Result<Vec<Account>> {
        let frames=self.call(Frame::new("accounts"))?;
        let accounts=parse_accounts(&frames)?;
        let result=accounts.iter().map(|(_,account)|account.clone()).collect();
        self.state.aqbanking.as_mut().unwrap().accounts=accounts; Ok(result)
    }
    fn account_request(&mut self,op:&str,account:&Account) -> Result<Frame> {
        if self.state.aqbanking.as_ref().unwrap().accounts.is_empty() { self.accounts()?; }
        let (id,_)=self.known_account(account)?;
        let mut frame=Frame::new(op); frame.put("account",id.to_string()); Ok(frame)
    }
    /// Refresh native capabilities without rediscovering other accounts or
    /// requesting their balances/transactions. Existing accesses need no login.
    pub fn current_accounts(&mut self,accounts:Vec<Account>) -> Result<Vec<Account>> {
        self.open()?;
        accounts.iter().map(|a| self.known_account(a).map(|(_,known)|known.clone())).collect()
    }
    fn known_account(&self,account:&Account) -> Result<(u32,&Account)> {
        let mut matching=self.state.aqbanking.as_ref().unwrap().accounts.iter().filter(|(_,known)| {
            if !account.iban.is_empty() { known.iban==account.iban && known.sub_account==account.sub_account }
            else { known.account_number==account.account_number && known.blz==account.blz && known.sub_account==account.sub_account }
        });
        let (id,known)=matching.next().ok_or_else(||Error::Other("Konto bei AqBanking nicht gefunden. Bankzugang synchronisieren.".into()))?;
        if matching.next().is_some() { return Err(Error::Security("Konto bei AqBanking nicht eindeutig".into())); }
        Ok((*id,known))
    }
    pub fn balance(&mut self,account:&Account) -> Result<Option<Balance>> {
        if !self.planned_reads.is_empty() || !self.prefetched_reads.is_empty() {
            if let Some(result)=self.cached_balance(account)? {return result;}
        }
        let request=self.account_request("balance",account)?; let frames=self.call(request)?; parse_balance(&frames)
    }
    pub fn transaction_report(&mut self,account:&Account,from:NaiveDate,to:Option<NaiveDate>) -> Result<TransactionReport> {
        if !self.planned_reads.is_empty() || !self.prefetched_reads.is_empty() {
            if let Some(result)=self.cached_transactions(account,from,to)? {return result;}
        }
        let mut request=self.account_request("transactions",account)?; date_field(&mut request,"from",Some(from)); date_field(&mut request,"to",to);
        let frames=self.call(request)?; parse_transaction_report(&frames)
    }

    fn send(&mut self,op:&str,account:&Account,transfer:&SepaTransfer,standing:Option<&StandingOrder>) -> Result<String> {
        if transfer.debtor_iban!=account.iban || transfer.amount<=Decimal::ZERO || transfer.amount.normalize().scale()>2 {
            return Err(Error::Security("Ungültiger Zahlungsauftrag".into()));
        }
        if transfer.instant && (op!="transfer" || transfer.execution_date.is_some()) {
            return Err(Error::Security("Echtzeitüberweisungen werden nur sofort als Einzelauftrag ausgeführt.".into()));
        }
        if (op=="scheduled" && transfer.execution_date.is_none_or(|d| d<=chrono::Local::now().date_naive()))
            || (op=="transfer" && transfer.execution_date.is_some()) {
            return Err(Error::Security("Ungültiges Ausführungsdatum für diese Überweisungsart.".into()));
        }
        self.payment_notice.clear();
        self.instant_result=None;
        self.instant_context=transfer.instant.then(||(account.iban.clone(),None));
        let mut request=self.account_request(op,account)?;
        request.put("name",&transfer.creditor_name); request.put("iban",&transfer.creditor_iban); request.put("bic",&transfer.creditor_bic);
        request.put("amount",transfer.amount.to_string()); request.put("purpose",&transfer.purpose); request.put("end_to_end",&transfer.end_to_end_id);
        request.put("instant",if transfer.instant {"1"} else {"0"}); date_field(&mut request,"date",transfer.execution_date);
        if let Some(standing)=standing {
            request.put("order_id",&standing.order_id); request.put("native",&standing.raw_pain);
            request.put("period",if standing.interval==Interval::Weekly {"weekly"} else {"monthly"});
            request.put("cycle",standing.every.to_string()); request.put("day",standing.day.to_string());
            date_field(&mut request,"first",Some(standing.first_date)); date_field(&mut request,"last",standing.last_date);
        }
        self.call(request)?;
        if transfer.instant {self.complete_instant(account)}
        else if op=="scheduled" {
            Ok(format!("Die Bank hat die Terminüberweisung zur späteren Ausführung angenommen.{}",
                if self.payment_notice.is_empty() {String::new()} else {format!("\n{}",self.payment_notice)}))
        } else {Ok("Die Bank hat den Auftrag angenommen.".into())}
    }
    pub fn transfer(&mut self,a:&Account,t:&SepaTransfer) -> Result<String> { self.send("transfer",a,t,None) }
    pub fn scheduled_transfer(&mut self,a:&Account,t:&SepaTransfer) -> Result<String> { self.send("scheduled",a,t,None) }
    pub fn create_standing_order(&mut self,a:&Account,s:&StandingOrder) -> Result<String> { self.send("create_standing",a,&s.transfer,Some(s)) }
    pub fn delete_standing_order(&mut self,a:&Account,s:&StandingOrder) -> Result<String> {
        if s.descriptor!="aqbanking-db-v1" { return Err(Error::Other("Daueraufträge vor dem Löschen erneut mit AqBanking abrufen.".into())); }
        self.send("delete_standing",a,&s.transfer,Some(s))
    }
    pub fn standing_orders(&mut self,account:&Account) -> Result<Vec<StandingOrder>> {
        let request=self.account_request("standing",account)?; let frames=self.call(request)?; let mut orders=Vec::new();
        for f in frames {
            if text(&f,"event")?!="transaction" { continue; }
            if number(&f,"type")?!=258 { return Err(invalid()); }
            if text(&f,"currency")?!="EUR" { return Err(Error::NotSupported("Dauerauftrag in Fremdwährung".into())); }
            orders.push(StandingOrder {order_id:text(&f,"order_id")?.into(),
                transfer:SepaTransfer {debtor_name:account.owner.clone(),debtor_iban:account.iban.clone(),debtor_bic:account.bic.clone(),
                    creditor_name:text(&f,"name")?.into(),creditor_iban:text(&f,"iban")?.into(),creditor_bic:text(&f,"bic")?.into(),
                    amount:amount(&f)?,purpose:text(&f,"purpose")?.into(),end_to_end_id:text(&f,"end_to_end")?.into(),execution_date:None,instant:false},
                first_date:date(&f,"first")?.ok_or_else(invalid)?,last_date:date(&f,"last")?,execution_time:None,
                interval:match number(&f,"period")? {1=>Interval::Monthly,2=>Interval::Weekly,_=>return Err(invalid())},
                every:u32::try_from(number(&f,"cycle")?).map_err(|_|invalid())?,day:u32::try_from(number(&f,"day")?).map_err(|_|invalid())?,
                raw_pain:f.bytes("native").to_vec(),descriptor:"aqbanking-db-v1".into()});
        }
        Ok(orders)
    }
}

fn local_client(medium:Medium, ui:&dyn Interaction) -> Result<Client<'_>> {
    Client::new(Config {url:"https://bank.invalid".into(),..Default::default()},State::default(),medium,ui)
}

/// Public bank details returned by Libchipcard; no key or cryptographic state.
#[derive(Debug, Clone)]
pub struct CardContext {
    pub record:u8, pub profile:u32, pub bank_name:String, pub blz:String,
    pub user_id:String, pub customer_id:String, pub host:String,
}

/// Read a bank context through Libchipcard's DDV/ZKA crypt token plugin.
pub fn read_card(medium:Medium,ui:&dyn Interaction) -> Result<(CardContext,String)> {
    let mut client=local_client(medium,ui)?;
    let frames=client.call(Frame::new("read_card"))?;
    let mut cards=frames.iter().filter(|f|f.text("event").ok()==Some("card"));
    let card=cards.next().ok_or_else(invalid)?;
    if cards.next().is_some() { return Err(invalid()); }
    Ok((CardContext {
        record:u8::try_from(number(card,"record")?).map_err(|_|invalid())?,
        profile:u32::try_from(number(card,"profile")?).map_err(|_|invalid())?,
        bank_name:text(card,"bank_name")?.into(),blz:text(card,"blz")?.into(),
        user_id:text(card,"user")?.into(),customer_id:text(card,"customer")?.into(),
        host:text(card,"url")?.into()
    },text(card,"number")?.into()))
}

pub fn chiptan_usb(reader:Option<&str>,payload:&[u8],ui:&dyn Interaction) -> Result<String> {
    if payload.is_empty() || payload.len()>247 { return Err(invalid()); }
    let mut client=local_client(Medium::Ddv {reader:reader.unwrap_or_default().into(),record:0},ui)?;
    let mut request=Frame::new("chiptan_usb"); request.put("payload",payload);
    let frames=client.call(request)?;
    let frame=frames.iter().find(|f|f.text("event").ok()==Some("tan")).ok_or_else(invalid)?;
    parse_tan(frame.bytes("response")).ok_or_else(invalid)
}
fn parse_tan(data:&[u8]) -> Option<String> {
    let n=usize::from(*data.first()?);
    if !(1..=16).contains(&n) {return None;}
    let mut tan=String::new();
    for (i,&b) in data.get(1..=n)?.iter().enumerate() {
        let hi=b>>4; let lo=b&15;
        if hi>9 || lo>9 && !(lo==15 && i+1==n) {return None;}
        tan.push(char::from(b'0'+hi));
        if lo<=9 {tan.push(char::from(b'0'+lo));}
    }
    Some(tan)
}


fn parse_transaction_report(frames:&[Frame]) -> Result<TransactionReport> {
    let balance=parse_balance(frames)?; let mut transactions=Vec::new();
    for f in frames {
        if text(f,"event")?!="transaction" { continue; }
        if !matches!(number(f,"type")?,128|129) { return Err(invalid()); }
        let booking=date(f,"date")?; let value=date(f,"value_date")?;
        let mut tx=Transaction {booking_date:booking.or(value).unwrap_or_default(),value_date:value.or(booking).unwrap_or_default(),
            date_unknown:booking.is_none() && value.is_none(),amount:amount(f)?,currency:text(f,"currency")?.into(),
            counterparty_name:text(f,"name")?.into(),counterparty_iban:text(f,"iban")?.into(),counterparty_bic:text(f,"bic")?.into(),
            purpose:text(f,"purpose")?.into(),posting_text:text(f,"posting")?.into(),end_to_end_id:text(f,"end_to_end")?.into(),
            mandate_id:text(f,"mandate")?.into(),creditor_id:text(f,"creditor")?.into(),
            pending:number(f,"type")?==129 || number(f,"status")?==6,fingerprint:String::new()};
        tx.compute_fingerprint(); transactions.push(tx);
    }
    Ok(TransactionReport {transactions,balance,warnings:Vec::new()})
}


fn parse_balance(frames:&[Frame]) -> Result<Option<Balance>> {
    let mut booked=None; let mut available=None;
    for frame in frames {
        if text(frame,"event")?!="balance" { continue; }
        match number(frame,"type")? {
            2=>{
                let balance=Balance {booked:amount(frame)?,available:None,currency:text(frame,"currency")?.into(),date:date(frame,"date")?.ok_or_else(invalid)?};
                if booked.as_ref().is_none_or(|old:&Balance|old.date<=balance.date) {booked=Some(balance);}
            },
            4=>available=Some((amount(frame)?,text(frame,"currency")?.to_owned())),
            _=>{},
        }
    }
    if let Some(balance)=&mut booked { if let Some((amount,currency))=available { if currency==balance.currency {balance.available=Some(amount);} } }
    Ok(booked)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    fn synthetic_card_worker(ui:&dyn Interaction,codes:Vec<(i64,bool)>) -> (CardWorker,std::thread::JoinHandle<Vec<String>>) {
        // cat only echoes our IPC into the fixture thread; no native library,
        // bank connection or physical reader is used by these Rust tests.
        let mut child=Command::new("/bin/cat").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let input=child.stdin.take().unwrap(); let mut output=child.stdout.take().unwrap();
        let (send,receive)=mpsc::sync_channel(8);
        let handle=std::thread::spawn(move || {
            let mut operations=Vec::new();
            for (code,reusable) in codes {
                let request=Frame::read(&mut output).unwrap();
                operations.push(request.text("op").unwrap().into());
                // Subsequent requests must not replace identity or native state.
                assert!(request.bytes("state").is_empty());
                assert!(request.bytes("user").is_empty());
                let mut done=Frame::new("done"); done.put("event","done");
                for (key,value) in [("code",code.to_string()),("bad_pin","0".into()),("cancelled","0".into()),
                    ("stage","balance".into()),("notice","Synthetic card result".into()),("state","synthetic-state".into()),
                    ("session",if reusable {"1"} else {"0"}.into()),("unsupported_account_job",if code == -68 {"1"} else {"0"}.into())] {
                    done.put(key,value);
                }
                send.send(Ok(done)).unwrap();
            }
            operations
        });
        (CardWorker {worker:Worker(child),input,receive,_blocker:tempfile::NamedTempFile::new().unwrap(),generation:ui.session_generation()},handle)
    }
    #[test]
    #[cfg(unix)]
    fn card_reads_reuse_one_worker_and_stop_after_a_bank_failure() {
        let ui=Capture::default();
        let mut client=local_client(Medium::Rdh{reader:String::new(),record:1},&ui).unwrap();
        let (worker,requests)=synthetic_card_worker(&ui,vec![(0,true),(-68,true),(-1,false)]);
        let pid=worker.worker.0.id(); client.session=Some(worker);
        client.call(Frame::new("open")).unwrap();
        assert_eq!(client.session.as_ref().unwrap().worker.0.id(),pid);
        assert!(matches!(client.call(Frame::new("transactions")),Err(Error::NotSupported(_))));
        assert_eq!(client.session.as_ref().unwrap().worker.0.id(),pid);
        assert!(matches!(client.call(Frame::new("balance")),Err(Error::Access{..})));
        assert!(client.session.is_none());
        assert_eq!(client.state.aqbanking.as_ref().unwrap().snapshot,b"synthetic-state");
        assert_eq!(requests.join().unwrap(),["open","transactions","balance"]);
    }
    #[test]
    #[cfg(unix)]
    fn card_session_is_dropped_when_the_vault_generation_changes() {
        let ui=Capture::default();
        let mut client=local_client(Medium::Ddv{reader:String::new(),record:1},&ui).unwrap();
        let (mut worker,requests)=synthetic_card_worker(&ui,vec![]);
        worker.generation=ui.session_generation()+1; client.session=Some(worker);
        assert!(matches!(client.call(Frame::new("open")),Err(Error::Cancelled)));
        assert!(client.session.is_none()); assert!(requests.join().unwrap().is_empty());
    }
    #[test]
    fn payments_and_card_management_never_reuse_read_sessions() {
        for op in ["transfer","scheduled","create_standing","delete_standing","read_card","chiptan_usb","reset_tan","configure"] {
            assert!(!read_operation(op));
        }
    }
    #[test]
    fn unsupported_account_jobs_keep_details_without_masking_setup_or_runtime_errors() {
        let mut frame=Frame::new("done");
        frame.put("stage","transactions"); frame.put("bad_pin","0");
        frame.put("notice","No matching native job for this account");
        // A generic library return code alone is not evidence that the bank
        // doesn't support transactions. Keep the original error information.
        assert!(matches!(native_error(&frame,-68).unwrap(),Error::Bank {text,..} if text.contains("No matching native job")));
        frame.put("unsupported_account_job","1");
        assert!(matches!(native_error(&frame,-68).unwrap(),Error::NotSupported(text) if text.contains("No matching native job")));
        frame.put("stage","getaccounts");
        assert!(matches!(native_error(&frame,-68).unwrap(),Error::Access {..}));
        frame.put("stage","transactions"); frame.put("bad_pin","1");
        assert!(matches!(native_error(&frame,-68).unwrap(),Error::Bank {code,..} if code=="9930"));
    }
    #[test]
    fn native_account_frames_replace_stale_permissions_and_keep_card_identity() {
        let mut frame=Frame::new("account"); frame.put("event","account");
        for (key,value) in [("id","23"),("kind","2"),("iban",""),("bic",""),("number","0001234567"),
            ("sub","01"),("blz","12345678"),("owner","Synthetic"),("name","Test card"),("currency","EUR"),("allowed","HKSAL\n")] {
            frame.put(key,value);
        }
        let accounts=parse_accounts(std::slice::from_ref(&frame)).unwrap();
        assert_eq!(accounts[0].0,23); assert_eq!(accounts[0].1.kind,50);
        assert_eq!(accounts[0].1.allowed,["HKSAL"]);
        let ui=Capture::default(); let mut client=local_client(Medium::PinTan,&ui).unwrap();
        let mut stale=accounts[0].1.clone(); stale.allowed.push("HKKAZ".into());
        client.state.aqbanking.as_mut().unwrap().accounts=accounts;
        assert_eq!(client.known_account(&stale).unwrap().1.allowed,["HKSAL"]);
        frame.put("allowed","");
        let accounts=parse_accounts(std::slice::from_ref(&frame)).unwrap();
        assert!(!accounts[0].1.allows("HKKAZ")); // empty must not mean unknown/all
        frame.put("id","0");
        assert!(parse_accounts(&[frame]).is_err());
    }
    #[test]
    fn device_name_uses_text_input_and_never_the_pin_or_tan_path() {
        struct NameInput(Option<&'static str>);
        impl Interaction for NameInput {
            fn pin(&self,_:&str)->Option<String> { panic!("device name is not a PIN") }
            fn tan(&self,_:&TanChallenge,_:Option<&[u8]>,_:Option<(&str,&[u8])>)->TanAnswer { panic!("device name is not a TAN") }
            fn progress(&self,_:&str) {}
            fn text(&self,title:&str,prompt:&str)->Option<String> {
                assert_eq!(title,"Name des TAN-Mediums"); assert!(prompt.contains("BestSign"));
                self.0.map(str::to_owned)
            }
        }
        let mut frame=Frame::new("password"); frame.put("event","password");
        frame.put("flags","2"); frame.put("title","Name des TAN-Mediums");
        frame.put("text","Exakter BestSign-Gerätename");
        for name in [Some(" BestSign Gerät "),None] {
            let ui=NameInput(name); let client=local_client(Medium::PinTan,&ui).unwrap();
            let reply=client.answer(&frame).unwrap();
            if let Some(name)=name { assert_eq!(reply.text("value").unwrap(),name); }
            else { assert_eq!(reply.text("cancel").unwrap(),"1"); }
        }
    }
    #[test]
    fn setup_failures_are_access_wide_and_preserve_the_original_bank_notice() {
        let mut frame=Frame::new("done");
        frame.put("bad_pin","0");
        frame.put("notice","HBCI: 9010 - Synthetic original rejection (S)\nHBCI: 9999 - Dialog bereits geschlossen. (S)");
        for stage in ["initialize","select_card_context","getbankinfo","getkeys","getsysid","select_tan","select_tan_medium","getitanmodes","configure_accounts","getaccounts","getaccsepa","accounts"] {
            frame.put("stage",stage);
            let Error::Access {code,text}=native_error(&frame,3).unwrap() else {panic!("must stop the access")};
            assert_eq!(code,"3");
            assert!(text.find("9010").unwrap()<text.find("9999").unwrap());
            assert!(text.contains("AqBanking-Status 3"));
        }
        frame.put("stage","balance");
        assert!(matches!(native_error(&frame,-1).unwrap(),Error::Bank{..}));
        frame.put("stage","getsysid"); frame.put("bad_pin","1");
        assert!(matches!(native_error(&frame,3).unwrap(),Error::Access{code,..} if code=="9930"));
        frame.put("notice","");
        assert!(native_error(&frame,3).unwrap().to_string().contains("Keine nähere Fehlermeldung"));
    }
    #[test]
    fn automatic_approval_waits_and_only_acknowledges_status_queries() {
        use std::sync::{Mutex,atomic::{AtomicBool,AtomicU64,Ordering}};
        struct Pending { updates:Mutex<Vec<u32>>, cancel:AtomicBool, generation:AtomicU64, stop:u8 }
        impl Interaction for Pending {
            fn pin(&self,_:&str)->Option<String> { panic!("no PIN") }
            fn tan(&self,_:&TanChallenge,_:Option<&[u8]>,_:Option<(&str,&[u8])>)->TanAnswer { panic!("no manual approval") }
            fn progress(&self,_:&str) {}
            fn cancelled(&self)->bool { self.cancel.load(Ordering::Acquire) }
            fn session_generation(&self)->u64 { self.generation.load(Ordering::Acquire) }
            fn approval_pending(&self,title:&str,text:&str,remaining:u32,attempt:u32,limit:u32) {
                assert_eq!((title,text,attempt,limit),("BestSign-Push","Synthetic approval",2,60));
                self.updates.lock().unwrap().push(remaining);
                if self.stop==1 { self.cancel.store(true,Ordering::Release); }
                if self.stop==2 { self.generation.store(1,Ordering::Release); }
            }
        }
        let mut frame=Frame::new("password");
        for (key,value) in [("flags","32"),("method","7"),("automatic","1"),("wait_seconds","1"),
            ("attempt","2"),("limit","60"),("title","BestSign-Push"),("text","Synthetic approval")] {frame.put(key,value);}
        for stop in 0..3 {
            let ui=Pending {updates:Mutex::default(),cancel:AtomicBool::new(false),generation:AtomicU64::new(0),stop};
            let client=local_client(Medium::PinTan,&ui).unwrap();
            let start=Instant::now();
            let result=client.wait_for_approval(&frame,start+Duration::from_secs(3),0);
            if stop==0 {
                assert!(start.elapsed()>=Duration::from_secs(1));
                let reply=result.unwrap(); assert!(reply.bytes("value").is_empty() && reply.bytes("cancel").is_empty());
                assert_eq!(*ui.updates.lock().unwrap(),vec![1,0]);
                for (key,value) in [("method","1"),("flags","2"),("wait_seconds","-1"),("wait_seconds","1000"),("attempt","61"),("limit","0")] {
                    let previous=frame.text(key).unwrap().to_owned(); frame.put(key,value);
                    assert!(client.wait_for_approval(&frame,Instant::now()+Duration::from_secs(3),0).is_err());
                    frame.put(key,previous);
                }
                assert!(matches!(client.wait_for_approval(&frame,Instant::now(),0),Err(Error::Transport(_))));
            } else { assert!(matches!(result,Err(Error::Cancelled))); }
        }
    }

    #[derive(Default)]
    struct Capture(std::sync::Mutex<Vec<u8>>);
    impl Interaction for Capture {
        fn pin(&self,_:&str)->Option<String>{None}
        fn progress(&self,_:&str){}
        fn tan(&self,_:&TanChallenge,hhd:Option<&[u8]>,_:Option<(&str,&[u8])>)->TanAnswer {
            *self.0.lock().unwrap()=hhd.unwrap_or_default().to_vec(); TanAnswer::Cancel
        }
    }
    #[test]
    fn registered_identity_replaces_legacy_values_for_every_banking_medium() {
        let ui=Capture::default();
        for medium in [Medium::PinTan,Medium::Ddv{reader:String::new(),record:1},Medium::Rdh{reader:String::new(),record:1}] {
            for legacy in ["", "LEGACYTESTPRODUCT", &"X".repeat(26)] {
                let config=Config{url:"https://bank.invalid".into(),product_id:legacy.into(),
                    product_version:legacy.into(),..Default::default()};
                let client=Client::new(config,State::default(),medium.clone(),&ui).unwrap();
                let mut request=Frame::new("open");
                client.configure_request(&mut request,product::REGISTRATION);
                let registration=request.bytes("product");
                assert_eq!(registration.len(),25);
                assert!(registration.iter().all(u8::is_ascii_alphanumeric));
                assert_ne!(registration,legacy.as_bytes());
                assert_eq!(request.bytes("product_version").len(),5);
                assert!(request.bytes("product_version").iter().all(u8::is_ascii_digit));
                assert_eq!(request.entries().filter(|(_,v)| v.windows(registration.len()).any(|w| w==registration))
                    .map(|(key,_)| key).collect::<Vec<_>>(),["product"]);
                assert_eq!(client.config.product_id,legacy); // no vault mutation
            }
        }
    }
    #[test]
    fn postbank_prefers_advertised_bestsign_only_on_its_exact_https_endpoint() {
        let ui=Capture::default();
        for (endpoint,expected) in [("https://hbci.postbank.de/banking/hbci.do","bestsign"),
            ("https://hbci.postbank.de.evil.invalid/",""),
            ("https://bank.invalid/BestSign","")] {
            let client=Client::new(Config{url:endpoint.into(),..Default::default()},State::default(),Medium::PinTan,&ui).unwrap();
            let mut frame=Frame::new("transfer"); client.configure_request(&mut frame,product::REGISTRATION);
            assert_eq!(frame.text("preferred_tan").unwrap_or(""),expected);
            assert_eq!(frame.text("skip_matching_vop").unwrap_or(""),if expected.is_empty() {""} else {"1"});
        }
    }
    #[test]
    fn push_approval_and_tan_entry_are_not_interchangeable() {
        struct Approval(bool,u8);
        impl Interaction for Approval {
            fn pin(&self,_:&str)->Option<String> {panic!("no PIN in approval")}
            fn progress(&self,_:&str) {}
            fn tan(&self,ch:&TanChallenge,_:Option<&[u8]>,_:Option<(&str,&[u8])>)->TanAnswer {
                assert_eq!(ch.decoupled,self.0);
                assert_eq!(ch.text,"Synthetic bank challenge");
                match self.1 {0=>TanAnswer::Confirmed,1=>TanAnswer::Tan("aB1234".into()),_=>TanAnswer::Cancel}
            }
        }
        let mut frame=Frame::new("password");
        frame.put("event","password"); frame.put("flags","162");
        frame.put("title","BestSign"); frame.put("text","Synthetic bank challenge"); frame.put("max","1");
        for (method,answer,expected) in [(7,0,Some("")),(7,2,None),(1,1,Some("aB1234")),(1,2,None)] {
            frame.put("method",method.to_string()); let ui=Approval(method==7,answer);
            let client=local_client(Medium::PinTan,&ui).unwrap(); let reply=client.answer(&frame).unwrap();
            if let Some(value)=expected {assert_eq!(reply.text("value").unwrap(),value);}
            else {assert_eq!(reply.text("cancel").unwrap(),"1");}
        }
        for (method,answer) in [(7,1),(1,0)] {
            frame.put("method",method.to_string()); let ui=Approval(method==7,answer);
            assert!(local_client(Medium::PinTan,&ui).unwrap().answer(&frame).is_err());
        }
    }
    #[test]
    fn native_challenge_is_exact_and_bad_inputs_never_reach_ui() {
        let ui=Capture::default(); let client=local_client(Medium::PinTan,&ui).unwrap();
        let mut frame=Frame::new("password");frame.put("method","3");frame.put("max","6");frame.put("challenge","0201AB");
        assert_eq!(client.answer_tan(&frame,"TAN","synthetic").unwrap(),None);
        assert_eq!(*ui.0.lock().unwrap(),[2,1,171]);
        for bad in ["0","0301AB","0G","äx"] {
            frame.put("challenge",bad); assert!(client.answer_tan(&frame,"TAN","synthetic").is_err());
            assert_eq!(*ui.0.lock().unwrap(),[2,1,171]);
        }
        frame.put("challenge","0201AB");frame.put("image",vec![0;1_048_577]);
        assert!(client.answer_tan(&frame,"TAN","synthetic").is_err());
    }
    #[test]
    fn native_state_is_bound_to_the_access_and_ambiguous_accounts_are_rejected() {
        let ui=Capture::default();let config=Config{url:"https://bank.invalid".into(),user_id:"one".into(),..Default::default()};
        let mut client=Client::new(config.clone(),State::default(),Medium::PinTan,&ui).unwrap();
        let account=Account{iban:"DE02120300000000202051".into(),..Default::default()};
        client.state.aqbanking.as_mut().unwrap().accounts=vec![(1,account.clone()),(2,account.clone())];
        assert!(client.account_request("balance",&account).is_err());
        client.state.aqbanking.as_mut().unwrap().snapshot=b"synthetic snapshot".to_vec();
        let changed=Config{user_id:"two".into(),..config.clone()};
        let reset=Client::new(changed,client.state.clone(),Medium::PinTan,&ui).unwrap();
        assert!(reset.state.aqbanking.as_ref().unwrap().snapshot.is_empty());
        client.state.aqbanking.as_mut().unwrap().version=99;
        assert!(Client::new(config,client.state,Medium::PinTan,&ui).is_err());
    }
    #[test]
    fn reject_inexact_money_and_malformed_tans() {
        for (value,expected) in [("123/100",Some("1.23")),("-12/10",Some("-1.2")),("1/3",None),("1/0",None)] {
            let mut f=Frame::new("test");f.put("amount",value);
            assert_eq!(amount(&f).ok().map(|d|d.to_string()),expected.map(str::to_owned));
        }
        assert_eq!(parse_tan(&[3,0x12,0x34,0x5f]),Some("12345".into()));
        for data in [&[][..],&[0],&[3,0x12],&[1,0xa1],&[2,0x1f,0x23],&[255]] {assert!(parse_tan(data).is_none());}
    }
}
