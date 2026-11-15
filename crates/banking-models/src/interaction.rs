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
    fn pin(&self, prompt: &str) -> Option<String>;
    /// AqBanking supplies the rendered HHD bytes or an image; the UI displays it.
    fn tan(&self, challenge:&TanChallenge, rendered_hhd:Option<&[u8]>, image:Option<(&str,&[u8])>) -> TanAnswer;
    fn progress(&self, text: &str);
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
