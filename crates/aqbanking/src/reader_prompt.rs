//! Reader input has its own deadline, independent of bank/progress timeouts.
use super::*;

pub(super) const PIN_TIMEOUT:Duration=Duration::from_secs(30);
pub(super) struct ReaderPrompt<'a> {
    ui:&'a dyn Interaction,
    deadline:Option<(u32,Instant)>,
    last_seconds:Option<u32>,
    visible:bool,
    timeout:Duration,
}
impl<'a> ReaderPrompt<'a> {
    pub fn new(ui:&'a dyn Interaction,timeout:Duration)->Self {
        Self {ui,deadline:None,last_seconds:None,visible:false,timeout}
    }
    pub fn active(&self)->bool {self.deadline.is_some()}
    pub fn show(&mut self,frame:&Frame,now:Instant)->Result<()> {
        let id=u32::try_from(number(frame,"id")?).map_err(|_|invalid())?;
        if id==0 {return Err(invalid());}
        if frame.bytes("kind")==b"reader_pin" {
            if let Some((current,_))=self.deadline {
                if current!=id {return Err(invalid());}
                // Repeated notifications cannot extend the input deadline.
            } else {self.deadline=Some((id,now+self.timeout));self.last_seconds=None;}
            self.visible=true;self.tick(now)?;
        } else if !self.active() {
            self.visible=true;
            self.ui.hardware_prompt(true,&banking::display::strip_html(text(frame,"text")?));
        }
        Ok(())
    }
    pub fn hide(&mut self,frame:&Frame)->Result<()> {
        let id=u32::try_from(number(frame,"id")?).map_err(|_|invalid())?;
        if self.deadline.is_none_or(|(current,_)|current==id) {self.clear();}
        Ok(())
    }
    pub fn tick(&mut self,now:Instant)->Result<()> {
        if let Some((_,deadline))=self.deadline {
            if now>=deadline {
                self.ui.reader_pin_timed_out();
                return Err(Error::Transport("PIN-Eingabe am Kartenleser nach 30 Sekunden abgebrochen. Es wurde keine abgeschlossene Eingabe gemeldet; der Banking-Prozess wurde beendet.".into()));
            }
            let remaining=deadline.saturating_duration_since(now);
            let seconds=(remaining.as_secs()+u64::from(remaining.subsec_nanos()!=0)) as u32;
            if self.last_seconds!=Some(seconds) {self.ui.reader_pin(seconds);self.last_seconds=Some(seconds);}
        }
        Ok(())
    }
    fn clear(&mut self) {
        self.deadline=None;self.last_seconds=None;
        if self.visible {self.ui.hardware_prompt(false,"");self.visible=false;}
    }
}
impl Drop for ReaderPrompt<'_> {fn drop(&mut self) {self.clear();}}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    #[derive(Default)]
    struct Ui(Mutex<Vec<String>>);
    impl Interaction for Ui {
        fn pin(&self,_:&str)->Option<String> {panic!("reader must own the PIN")}
        fn tan(&self,_:&TanChallenge,_:Option<&[u8]>,_:Option<(&str,&[u8])>)->TanAnswer {panic!("no TAN")}
        fn progress(&self,_:&str) {}
        fn hardware_prompt(&self,show:bool,_:&str) {self.0.lock().unwrap().push(if show {"notice"} else {"hide"}.into());}
        fn reader_pin(&self,remaining:u32) {self.0.lock().unwrap().push(remaining.to_string());}
        fn reader_pin_timed_out(&self) {self.0.lock().unwrap().push("timeout".into());}
    }
    fn notification(event:&str,id:u32)->Frame {
        let mut f=Frame::new(event);f.put("event",event);f.put("id",id.to_string());f.put("kind","reader_pin");f.put("text","Synthetic instruction");f
    }
    #[test]
    fn countdown_is_thirty_seconds_and_only_matching_completion_stops_it() {
        assert_eq!(PIN_TIMEOUT,Duration::from_secs(30));
        let ui=Ui::default();let now=Instant::now();
        {
            let mut prompt=ReaderPrompt::new(&ui,PIN_TIMEOUT);
            prompt.show(&notification("show",7),now).unwrap();
            prompt.tick(now+Duration::from_secs(29)).unwrap();
            prompt.show(&notification("show",7),now+Duration::from_secs(29)).unwrap();
            prompt.hide(&notification("hide",8)).unwrap(); // unrelated event cannot dismiss/reset the PIN deadline
            assert!(matches!(prompt.tick(now+Duration::from_secs(30)),Err(Error::Transport(_))));
        }
        assert_eq!(*ui.0.lock().unwrap(),["30","1","timeout","hide"]);
        {
            let mut prompt=ReaderPrompt::new(&ui,PIN_TIMEOUT);
            prompt.show(&notification("show",9),now).unwrap();
            prompt.hide(&notification("hide",9)).unwrap();
            prompt.tick(now+Duration::from_secs(60)).unwrap();
            let mut notice=notification("show",10);notice.put("kind","notice");notice.put("text","PIN-like text is not a reader event");
            prompt.show(&notice,now).unwrap();prompt.tick(now+Duration::from_secs(60)).unwrap();
        }
        assert_eq!(*ui.0.lock().unwrap(),["30","1","timeout","hide","30","hide","notice","hide"]);
    }
    #[test]
    #[cfg(unix)]
    fn reader_timeout_kills_the_worker_and_clears_the_dialog_without_an_answer_or_retry() {
        for completed in [false,true] {
            let ui=Ui::default();
            let mut client=local_client(Medium::Rdh {reader:String::new(),record:1},&ui).unwrap();
            let mut child=Command::new("/bin/cat").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
            let input=child.stdin.take().unwrap();let mut output=child.stdout.take().unwrap();
            let (send,receive)=mpsc::sync_channel(8);
            let fixture=std::thread::spawn(move || {
                let request=Frame::read(&mut output).unwrap();assert_eq!(request.text("op").unwrap(),"open");
                send.send(Ok(notification("show",1))).unwrap();
                let mut progress=Frame::new("progress");progress.put("event","progress");progress.put("text","Synthetic progress");send.send(Ok(progress)).unwrap();
                if completed {
                    send.send(Ok(notification("hide",1))).unwrap();
                    let mut done=Frame::new("done");
                    for (k,v) in [("event","done"),("code","0"),("bad_pin","0"),("cancelled","0"),("session","1"),("state","synthetic-state")] {done.put(k,v);}
                    send.send(Ok(done)).unwrap();
                }
                // Keep the receive channel alive while the helper is waiting.
                // On timeout the parent's worker kill must close stdout with no
                // answer, new request or automatic retry. Success closes normally.
                let next=Frame::read(&mut output);
                if completed {assert_eq!(next.unwrap().text("op").unwrap(),"close");} else {assert!(next.is_err());}
                drop(send);
            });
            client.session=Some(CardWorker {worker:Worker(child),input,receive,_blocker:tempfile::NamedTempFile::new().unwrap(),generation:0});
            let start=Instant::now();
            let result=client.call_with_reader_timeout(Frame::new("open"),Duration::from_millis(100));
            if completed {assert!(result.is_ok());client.finish();}
            else {
                assert!(matches!(result,Err(Error::Transport(text)) if text.contains("30 Sekunden")));
                assert!(client.session.is_none());assert!(start.elapsed()<Duration::from_secs(3));
            }
            fixture.join().unwrap();
            let events=ui.0.lock().unwrap();assert_eq!(events.first().map(String::as_str),Some("1"));assert_eq!(events.last().map(String::as_str),Some("hide"));
        }
    }
}
