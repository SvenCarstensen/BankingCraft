use crate::model::*;

/// Callbacks into the UI. Called from the worker thread and may block.
pub trait Interaction: Send + Sync {
    fn cancelled(&self) -> bool { false }
    /// Revokes responses from workers belonging to an earlier unlocked session.
    fn session_generation(&self) -> u64 { 0 }
    fn pin_rejected(&self) {}
    fn secret(&self,_title:&str,prompt:&str,cache:bool) -> Option<String> { if cache {self.pin(prompt)} else {None} }
    fn select(&self,_title:&str,_text:&str,_options:&[(String,String)]) -> Option<String> { None }
    fn hardware_prompt(&self,show:bool,text:&str) { if show {self.progress(text);} }
    /// Stop the enclosing synchronization after its reader input expires.
    fn reader_pin_timed_out(&self) {}
    /// Nonblocking reader-PIN countdown; the PIN remains on the reader.
    fn reader_pin(&self, _remaining:u32) { self.hardware_prompt(true, i18n::tr("Bitte geben Sie die PIN Ihrer HBCI-Chipkarte am Kartenleser ein und bestätigen Sie mit OK.")); }
    fn pin(&self, prompt: &str) -> Option<String>;
    /// AqBanking supplies the rendered HHD bytes or an image; the UI displays it.
    fn tan(&self, challenge:&TanChallenge, rendered_hhd:Option<&[u8]>, image:Option<(&str,&[u8])>) -> TanAnswer;
    fn progress(&self, text: &str);
    /// Nonblocking update while the adapter waits to query a decoupled approval.
    fn approval_pending(&self, _title: &str, text: &str, _remaining: u32, _attempt: u32, _limit: u32) { self.progress(text); }
    /// Free text input (e.g. the name of a TAN medium). `None` = cancelled / unsupported.
    fn text(&self, _title: &str, _prompt: &str) -> Option<String> {
        None
    }
    /// The exact bank result must be shown before payment authorization. Default: decline.
    fn verify_payee(&self, _result: &crate::payee::PayeeVerification, _already_executed: bool) -> bool { false }
    /// Yes/no question (e.g. confirming a bank key fingerprint). Default: no.
    fn confirm(&self, _title: &str, _text: &str) -> bool {
        false
    }
}
