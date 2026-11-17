//! Blocking bridge between the FinTS worker thread and the egui dialogs.

use crate::{AppState, AppWindow, ChoiceRow};
use banking::{Interaction, TanAnswer, TanChallenge, TanKind};
use crate::ui::{ModelRc, SharedPixelBuffer, Rgba8Pixel, SharedString, VecModel};
use std::cell::{Cell, RefCell};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use zeroize::Zeroizing;

pub enum Answer {
    Text(String),
    Confirmed,
    Cancel,
}

#[derive(Clone)]
pub struct Bridge {
    pub weak: crate::ui::Weak<AppWindow>,
    /// Sync jobs publish account/operation progress themselves. This setting is
    /// local to a client clone; prompts and cancellation still share the session.
    native_progress: bool,
    /// Cancellation is control flow, independent of the translated message.
    pub cancelled: Arc<std::sync::atomic::AtomicBool>,
    pub locked: Arc<std::sync::atomic::AtomicBool>,
    pub generation: Arc<std::sync::atomic::AtomicU64>,
    pub prompt_id: Arc<std::sync::atomic::AtomicU64>,
    pub pin_was_rejected: Arc<std::sync::atomic::AtomicBool>,
    pub pending: Arc<Mutex<Option<mpsc::Sender<Answer>>>>,
    /// HHDuc bytes for chipTAN USB while a TAN dialog is open.
    pub usb_payload: Arc<Mutex<Option<Vec<u8>>>>,
    /// PIN cached for the running job (cleared on wrong PIN).
    pub pin_cache: Arc<Mutex<Option<Zeroizing<String>>>>,
    /// The user ticked "PIN speichern" for the PIN in `pin_cache`.
    pub pin_save_requested: Arc<Mutex<bool>>,
    /// Offer the "PIN speichern" option in the next PIN prompt.
    pub pin_save_offer: Arc<Mutex<bool>>,
}

thread_local! {
    static FLICKER_TIMER: RefCell<Option<crate::ui::Timer>> = const { RefCell::new(None) };
    static FLICKER_FRAMES: RefCell<Vec<[bool; 5]>> = const { RefCell::new(Vec::new()) };
    static FLICKER_POS: Cell<usize> = const { Cell::new(0) };
}

/// (Re)start the flicker animation with the current frequency. Main thread only.
pub fn restart_flicker(app: &AppWindow) {
    let hz = app.global::<AppState>().get_flicker_hz().clamp(2, 40) as u64;
    let weak = app.as_weak();
    let timer = crate::ui::Timer::default();
    timer.start(crate::ui::TimerMode::Repeated, std::time::Duration::from_millis(1000 / hz), move || {
        let Some(app) = weak.upgrade() else { return };
        FLICKER_FRAMES.with(|f| {
            let f = f.borrow();
            if f.is_empty() {
                return;
            }
            let pos = FLICKER_POS.with(|p| {
                let v = p.get();
                p.set((v + 1) % f.len());
                v
            });
            let fr = f[pos % f.len()];
            let st = app.global::<AppState>();
            st.set_flick0(fr[0]);
            st.set_flick1(fr[1]);
            st.set_flick2(fr[2]);
            st.set_flick3(fr[3]);
            st.set_flick4(fr[4]);
        });
    });
    FLICKER_TIMER.with(|t| *t.borrow_mut() = Some(timer));
}

pub fn stop_flicker() {
    FLICKER_TIMER.with(|t| *t.borrow_mut() = None);
    FLICKER_FRAMES.with(|f| f.borrow_mut().clear());
}

impl Bridge {
    pub fn new(weak: crate::ui::Weak<AppWindow>) -> Self {
        Self {
            weak,
            native_progress: true,
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            locked: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            prompt_id: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            pin_was_rejected: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            pending: Arc::new(Mutex::new(None)),
            usb_payload: Arc::new(Mutex::new(None)),
            pin_cache: Arc::new(Mutex::new(None)),
            pin_save_requested: Arc::new(Mutex::new(false)),
            pin_save_offer: Arc::new(Mutex::new(false)),
        }
    }

    fn ask(&self, setup: impl FnOnce(&AppWindow) + Send + 'static) -> Answer {
        use std::sync::atomic::Ordering;
        if self.locked.load(Ordering::Acquire) || self.cancelled.load(Ordering::Acquire) { return Answer::Cancel; }
        let epoch = self.session_generation();
        let prompt = self.prompt_id.fetch_add(1, Ordering::AcqRel) + 1;
        let (tx, rx) = mpsc::channel();
        *self.pending.lock().unwrap() = Some(tx);
        let locked = self.locked.clone();
        let generation = self.generation.clone();
        let prompt_id = self.prompt_id.clone();
        if self.weak.upgrade_in_event_loop(move |app| {
            if !locked.load(Ordering::Acquire) && generation.load(Ordering::Acquire) == epoch && prompt_id.load(Ordering::Acquire) == prompt {
                let st=app.global::<AppState>();
                if st.get_tan_kind()==4 { st.set_show_tan(false); }
                setup(&app);
            }
        }).is_err() {
            self.pending.lock().unwrap().take();
            return Answer::Cancel;
        }
        loop {
            if self.locked.load(Ordering::Acquire) || self.cancelled.load(Ordering::Acquire) || self.session_generation() != epoch {
                self.pending.lock().unwrap().take(); return Answer::Cancel;
            }
            match rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(answer) => return answer,
                Err(mpsc::RecvTimeoutError::Disconnected) => return Answer::Cancel,
                Err(mpsc::RecvTimeoutError::Timeout) => {},
            }
        }
    }

    /// Called from UI callbacks.
    pub fn answer(&self, a: Answer) {
        self.prompt_id.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        if let Some(tx) = self.pending.lock().unwrap().take() {
            let _ = tx.send(a);
        }
    }

    pub fn status(&self, text: &str) {
        let t = SharedString::from(text);
        let locked = self.locked.clone();
        let generation = self.generation.clone();
        let epoch = self.session_generation();
        let _ = self.weak.upgrade_in_event_loop(move |app| {
            if locked.load(std::sync::atomic::Ordering::Acquire) || generation.load(std::sync::atomic::Ordering::Acquire) != epoch { return; }
            app.global::<AppState>().set_busy_text(t);
        });
    }

    pub fn with_application_progress(&self) -> Self {
        Self { native_progress: false, ..self.clone() }
    }

    pub fn pinpad(&self, show: bool, text: &str) {
        self.reader_notice(show,text,-1);
    }
    fn reader_notice(&self, show:bool, text:&str, seconds:i32) {
        let t = SharedString::from(text);
        let locked = self.locked.clone();
        let generation = self.generation.clone();
        let epoch = self.session_generation();
        let _ = self.weak.upgrade_in_event_loop(move |app| {
            if locked.load(std::sync::atomic::Ordering::Acquire) || generation.load(std::sync::atomic::Ordering::Acquire) != epoch { return; }
            let st = app.global::<AppState>();
            if show && (!st.get_show_pinpad() || (seconds>=0)!=(st.get_pinpad_seconds()>=0) || st.get_pinpad_text()!=t) {
                crate::notifications::reader_requested(seconds>=0);
            } else if !show { crate::notifications::clear_reader(); }
            st.set_pinpad_text(t);
            st.set_pinpad_seconds(seconds);
            st.set_show_pinpad(show);
        });
    }

    pub fn ask_input(&self, heading: &str, prompt: &str, masked: bool) -> Option<String> {
        self.ask_input_ext(heading, prompt, masked, false)
    }

    fn ask_input_ext(&self, heading: &str, prompt: &str, masked: bool, offer_save: bool) -> Option<String> {
        let p = SharedString::from(prompt);
        let h = SharedString::from(heading);
        match self.ask(move |app| {
            let st = app.global::<AppState>();
            st.set_pin_heading(h);
            st.set_pin_masked(masked);
            st.set_pin_offer_save(offer_save);
            st.set_pin_save(false);
            st.set_pin_title(p);
            st.set_pin_value(SharedString::default());
            st.set_show_pin(true);
        }) {
            Answer::Text(t) if !t.is_empty() => Some(t),
            _ => None,
        }
    }

    pub fn choose(&self, title: &str, rows: Vec<(String, String, String)>) -> Option<String> {
        let title = SharedString::from(title);
        let rows: Vec<ChoiceRow> = rows
            .into_iter()
            .map(|(k, t, s)| ChoiceRow { key: k, title: t, subtitle: s })
            .collect();
        match self.ask(move |app| {
            let st = app.global::<AppState>();
            st.set_choice_title(title);
            st.set_choices(ModelRc::new(VecModel::from(rows)));
            st.set_show_choice(true);
        }) {
            Answer::Text(k) => Some(k),
            _ => None,
        }
    }

    pub fn has_reader(&self) -> bool {
        aqbanking::list_readers().map(|r| !r.is_empty()).unwrap_or(false)
    }
}

impl Interaction for Bridge {
    fn session_generation(&self) -> u64 { self.generation.load(std::sync::atomic::Ordering::Acquire) }
    fn pin_rejected(&self) {
        self.pin_was_rejected.store(true,std::sync::atomic::Ordering::Release);
        *self.pin_cache.lock().unwrap()=None;
        *self.pin_save_requested.lock().unwrap()=false;
    }
    fn secret(&self,title:&str,prompt:&str,cache:bool) -> Option<String> {
        if cache { self.pin(prompt) } else { self.ask_input(title,prompt,true) }
    }
    fn select(&self,title:&str,text:&str,options:&[(String,String)]) -> Option<String> {
        self.choose(&format!("{title}\n\n{text}"),options.iter().map(|(id,label)|(id.clone(),label.clone(),String::new())).collect())
    }
    fn hardware_prompt(&self,show:bool,text:&str) { self.pinpad(show,text); }
    fn reader_pin_timed_out(&self) { self.cancelled.store(true,std::sync::atomic::Ordering::Release); }
    fn reader_pin(&self,remaining:u32) {
        self.reader_notice(true,i18n::tr("Bitte geben Sie die PIN Ihrer HBCI-Chipkarte am Kartenleser ein und bestätigen Sie mit OK."),remaining as i32);
    }
    fn cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Acquire) || self.locked.load(std::sync::atomic::Ordering::Acquire)
    }
    fn pin(&self, prompt: &str) -> Option<String> {
        if self.cancelled() { return None; }
        if let Some(p) = self.pin_cache.lock().unwrap().clone() {
            return Some(p.to_string());
        }
        let offer = *self.pin_save_offer.lock().unwrap();
        let p = self.ask_input_ext(i18n::tr("PIN-Eingabe"), prompt, true, offer)?;
        *self.pin_cache.lock().unwrap() = Some(Zeroizing::new(p.clone()));
        Some(p)
    }

    fn tan(&self,ch:&TanChallenge,rendered:Option<&[u8]>,image:Option<(&str,&[u8])>)->TanAnswer {self.render_tan(ch,rendered,image)}

    fn approval_pending(&self,title:&str,text:&str,remaining:u32,attempt:u32,limit:u32) {
        use std::sync::atomic::Ordering;
        let title=SharedString::from(format!("Freigabe · {title}"));
        let text=SharedString::from(text);
        let status=if remaining==0 { format!("Freigabestatus wird abgefragt … ({attempt}/{limit})") }
            else { format!("Warte auf Freigabe. Nächste Statusabfrage in {remaining} s. ({attempt}/{limit})") };
        let bridge=self.clone(); let epoch=self.session_generation();
        let _=self.weak.upgrade_in_event_loop(move |app| {
            if bridge.cancelled() || bridge.locked.load(Ordering::Acquire) || bridge.session_generation()!=epoch { return; }
            let st=app.global::<AppState>();
            st.set_tan_kind(4); st.set_tan_title(title); st.set_tan_text(text);
            st.set_tan_status(status); st.set_tan_value(Default::default());
            st.set_tan_usb_possible(false); st.set_show_tan(true);
        });
    }

    fn progress(&self, text: &str) {
        if self.native_progress { self.status(text); }
    }

    fn text(&self, title: &str, prompt: &str) -> Option<String> {
        self.ask_input(title, prompt, false)
    }

    fn verify_payee(&self, result: &banking::payee::PayeeVerification, already_executed: bool) -> bool {
        let result = result.clone();
        matches!(self.ask(move |app| crate::payee_ui::show(app, &result, already_executed)), Answer::Confirmed)
    }

    fn confirm(&self, title: &str, text: &str) -> bool {
        let (t, x, b) = (title.to_string(), text.to_string(), self.clone());
        matches!(self.ask(move |app| crate::confirm_blocking(app, &t, &x, b)), Answer::Confirmed)
    }
}

impl Bridge {
    fn render_tan(&self, ch: &TanChallenge, rendered:Option<&[u8]>, native_image:Option<(&str,&[u8])>) -> TanAnswer {
        let text = banking::display::strip_html(&ch.text);
        let matrix = native_image.map(|(mime,data)|(mime.to_owned(),data.to_vec()));
        let kind = if ch.decoupled {
            3
        } else if matrix.is_some() {
            2
        } else if rendered.is_some() && ch.method.kind() != TanKind::Matrix {
            1
        } else {
            0
        };
        let frames = if kind == 1 { rendered.map(rendered_hhd_frames).unwrap_or_default() } else { vec![] };
        let usb = if kind == 1 { rendered.map(<[u8]>::to_vec) } else { None };
        let usb_possible = usb.is_some() && self.has_reader();
        *self.usb_payload.lock().unwrap() = usb;
        let pixels = matrix.as_ref().and_then(|(_, img)| decode_tan_image(img));
        if matrix.is_some() && pixels.is_none() { return TanAnswer::Cancel; }
        let title = SharedString::from(i18n::tr_format!("Freigabe · {}", ch.method.name));
        let text = SharedString::from(text);
        let answer = self.ask(move |app| {
            let st = app.global::<AppState>();
            st.set_tan_title(title);
            st.set_tan_text(text);
            st.set_tan_kind(kind);
            st.set_tan_value(SharedString::default());
            st.set_tan_usb_possible(usb_possible);
            if let Some(p) = pixels {
                st.set_tan_image(crate::ui::Image::from_rgba8(p));
            }
            if !frames.is_empty() {
                FLICKER_FRAMES.with(|f| *f.borrow_mut() = frames);
                FLICKER_POS.with(|p| p.set(0));
                restart_flicker(app);
            }
            st.set_show_tan(true);
        });
        *self.usb_payload.lock().unwrap() = None;
        match answer {
            Answer::Text(t) => TanAnswer::Tan(t),
            Answer::Confirmed => TanAnswer::Confirmed,
            Answer::Cancel => TanAnswer::Cancel,
        }
    }

}

fn rendered_hhd_frames(payload:&[u8])->Vec<[bool;5]> {
    [0x0f,0xff].iter().chain(payload).flat_map(|b| [b&15,b>>4]).flat_map(|n| [true,false].map(|clock|[clock,n&1!=0,n&2!=0,n&4!=0,n&8!=0])).collect()
}

fn decode_tan_image(bytes: &[u8]) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    if bytes.len() > 1_048_576 { return None; }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(2048);
    limits.max_image_height = Some(2048);
    limits.max_alloc = Some(16 * 1024 * 1024);
    reader.limits(limits);
    let rgba = reader.decode().ok()?.to_rgba8();
    Some(SharedPixelBuffer::clone_from_slice(rgba.as_raw(), rgba.width(), rgba.height()))
}

#[cfg(test)]
mod image_security_tests {
    use super::*;
    #[test]
    fn native_hhd_bytes_are_not_encoded_twice() {
        let payload=crate::demo::DEMO_FLICKER;
        let frames=rendered_hhd_frames(payload);
        let mut decoded=Vec::new();
        for chunk in frames.as_chunks::<4>().0.iter().skip(2) {
            assert!(chunk[0][0] && !chunk[1][0] && chunk[2][0] && !chunk[3][0]);
            let nibble=|frame:[bool;5]|(1..=4).fold(0u8,|n,i|n|((frame[i] as u8)<<(i-1)));
            decoded.push(nibble(chunk[0]) | (nibble(chunk[2])<<4));
        }
        assert_eq!(decoded,payload);
    }
    #[test]
    fn queued_prompts_cannot_reappear_in_a_new_session() {
        use std::sync::atomic::Ordering;
        let app = AppWindow::new().unwrap();
        let bridge = Bridge::new(app.as_weak());
        bridge.status("old account details");
        bridge.pinpad(true, "old reader prompt");
        bridge.generation.fetch_add(1, Ordering::AcqRel);
        app.pump(&egui::Context::default());
        assert_ne!(app.global::<AppState>().get_busy_text(), "old account details");
        assert!(!app.global::<AppState>().get_show_pinpad());
        let worker_bridge = bridge.clone();
        let worker = std::thread::spawn(move || worker_bridge.ask_input("old secret", "old prompt", true));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while bridge.pending.lock().unwrap().is_none() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        bridge.generation.fetch_add(1, Ordering::AcqRel);
        app.pump(&egui::Context::default());
        assert_eq!(worker.join().unwrap(), None);
        assert!(!app.global::<AppState>().get_show_pin());
    }
    #[test]
    fn tan_image_decodes_normal_image_but_rejects_excessive_dimensions_and_input() {
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 2).write_to(&mut encoded, image::ImageFormat::Bmp).unwrap();
        let bytes = encoded.into_inner();
        let pixels = decode_tan_image(&bytes).unwrap(); assert_eq!((pixels.width(), pixels.height()), (2, 2));
        let mut oversized = bytes.clone(); oversized[18..22].copy_from_slice(&100_000i32.to_le_bytes());
        assert!(decode_tan_image(&oversized).is_none());
        assert!(decode_tan_image(&vec![0; 1_048_577]).is_none());
        assert!(decode_tan_image(&bytes[..20]).is_none());
    }
}
