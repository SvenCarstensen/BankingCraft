//! Local macOS alerts. Only fixed, translated messages cross the native boundary;
//! no bank replies, account identifiers, credentials or challenge text are sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Success,
    Error,
    Cancelled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    ReaderPin,
    ReaderConfirm,
    Sync(Outcome),
    ClearReader,
}

#[cfg(any(target_os = "macos", test))]
impl Event {
    fn text(self) -> (&'static str, &'static str) {
        let (title,body)=match self {
            Self::ReaderPin=>("Eingabe am Kartenleser erforderlich","Bitte die PIN direkt am Kartenleser eingeben und dort bestätigen. Der Dialog wartet höchstens 30 Sekunden."),
            Self::ReaderConfirm=>("Bestätigung am Kartenleser erforderlich","Bitte die Anzeige am Kartenleser prüfen und die erforderliche Eingabe oder Bestätigung dort vornehmen."),
            Self::Sync(Outcome::Success)=>("Synchronisierung abgeschlossen","Die Synchronisierung wurde erfolgreich beendet."),
            Self::Sync(Outcome::Error)=>("Synchronisierung mit Fehlern beendet","Die Synchronisierung wurde nicht vollständig abgeschlossen. Einzelheiten stehen in BankingCraft."),
            Self::Sync(Outcome::Cancelled)=>("Synchronisierung abgebrochen","Der Vorgang wurde beendet. Bereits übermittelte Aufträge sind dadurch nicht storniert."),
            Self::ClearReader=>("",""),
        };
        (i18n::tr(title), i18n::tr(body))
    }
}

pub fn init() {
    #[cfg(all(target_os = "macos", not(test)))]
    {
        let title = cstring(i18n::tr("Mitteilungen aktiviert"));
        let body=cstring(i18n::tr("BankingCraft meldet das Ende der Synchronisierung und notwendige Eingaben am Kartenleser mit einem dezenten Signalton."));
        unsafe {
            bankingcraft_notifications_init(title.as_ptr(), body.as_ptr());
        }
    }
}
pub fn reader_requested(pin: bool) {
    emit(if pin {
        Event::ReaderPin
    } else {
        Event::ReaderConfirm
    });
}
pub fn clear_reader() {
    emit(Event::ClearReader);
}
pub fn sync_finished(outcome: Outcome) {
    emit(Event::Sync(outcome));
}
fn emit(event: Event) {
    #[cfg(test)]
    EVENTS.with(|events| events.borrow_mut().push(event));
    #[cfg(all(target_os = "macos", not(test)))]
    {
        if event == Event::ClearReader {
            unsafe {
                bankingcraft_notifications_clear_reader();
            }
            return;
        }
        let (title, body) = event.text();
        let title = cstring(title);
        let body = cstring(body);
        let reader = matches!(event, Event::ReaderPin | Event::ReaderConfirm);
        unsafe {
            bankingcraft_notifications_post(
                reader,
                title.as_ptr(),
                body.as_ptr(),
                if event == Event::ReaderPin { 30 } else { 180 },
            );
        }
    }
    // Other platforms retain their existing in-app prompts. The native notification
    // implementation is macOS-specific and does not introduce shell fallbacks.
    #[cfg(all(not(target_os = "macos"), not(test)))]
    let _ = event;
}
#[cfg(all(target_os = "macos", not(test)))]
fn cstring(s: &str) -> std::ffi::CString {
    std::ffi::CString::new(s).expect("static notification text contains no NUL")
}
#[cfg(all(target_os = "macos", not(test)))]
unsafe extern "C" {
    fn bankingcraft_notifications_init(
        title: *const std::ffi::c_char,
        body: *const std::ffi::c_char,
    );
    fn bankingcraft_notifications_post(
        reader: bool,
        title: *const std::ffi::c_char,
        body: *const std::ffi::c_char,
        lifetime: u32,
    );
    fn bankingcraft_notifications_clear_reader();
}
#[cfg(test)]
thread_local! {static EVENTS:std::cell::RefCell<Vec<Event>>=const{std::cell::RefCell::new(Vec::new())};}
#[cfg(test)]
mod tests;
