//! Blocking bridge between the FinTS worker thread and the Slint dialogs.

use crate::{AppState, AppWindow, ChoiceRow};
use fints::tan::{parse_matrix_code, FlickerCode};
use fints::{Interaction, TanAnswer, TanChallenge, TanKind, TanMethod};
use slint::{ComponentHandle, ModelRc, SharedPixelBuffer, Rgba8Pixel, SharedString, VecModel};
use std::cell::{Cell, RefCell};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

pub enum Answer {
    Text(String),
    Confirmed,
    Cancel,
}

#[derive(Clone)]
pub struct Bridge {
    pub weak: slint::Weak<AppWindow>,
    pub pending: Arc<Mutex<Option<mpsc::Sender<Answer>>>>,
    /// HHDuc bytes for chipTAN USB while a TAN dialog is open.
    pub usb_payload: Arc<Mutex<Option<Vec<u8>>>>,
    /// PIN cached for the running job (cleared on wrong PIN).
    pub pin_cache: Arc<Mutex<Option<String>>>,
}

thread_local! {
    static FLICKER_TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
    static FLICKER_FRAMES: RefCell<Vec<[bool; 5]>> = const { RefCell::new(Vec::new()) };
    static FLICKER_POS: Cell<usize> = const { Cell::new(0) };
}

/// (Re)start the flicker animation with the current frequency. Main thread only.
pub fn restart_flicker(app: &AppWindow) {
    let hz = app.global::<AppState>().get_flicker_hz().clamp(2, 40) as u64;
    let weak = app.as_weak();
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(1000 / hz), move || {
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
    pub fn new(weak: slint::Weak<AppWindow>) -> Self {
        Self {
            weak,
            pending: Arc::new(Mutex::new(None)),
            usb_payload: Arc::new(Mutex::new(None)),
            pin_cache: Arc::new(Mutex::new(None)),
        }
    }

    fn ask(&self, setup: impl FnOnce(&AppWindow) + Send + 'static) -> Answer {
        let (tx, rx) = mpsc::channel();
        *self.pending.lock().unwrap() = Some(tx);
        if self.weak.upgrade_in_event_loop(move |app| setup(&app)).is_err() {
            return Answer::Cancel;
        }
        rx.recv().unwrap_or(Answer::Cancel)
    }

    /// Called from UI callbacks.
    pub fn answer(&self, a: Answer) {
        if let Some(tx) = self.pending.lock().unwrap().take() {
            let _ = tx.send(a);
        }
    }

    pub fn status(&self, text: &str) {
        let t = SharedString::from(text);
        let _ = self.weak.upgrade_in_event_loop(move |app| {
            app.global::<AppState>().set_busy_text(t);
        });
    }

    pub fn pinpad(&self, show: bool, text: &str) {
        let t = SharedString::from(text);
        let _ = self.weak.upgrade_in_event_loop(move |app| {
            let st = app.global::<AppState>();
            st.set_pinpad_text(t);
            st.set_show_pinpad(show);
        });
    }

    pub fn ask_pin(&self, prompt: &str) -> Option<String> {
        self.ask_input("PIN-Eingabe", prompt, true)
    }

    pub fn ask_input(&self, heading: &str, prompt: &str, masked: bool) -> Option<String> {
        let p = SharedString::from(prompt);
        let h = SharedString::from(heading);
        match self.ask(move |app| {
            let st = app.global::<AppState>();
            st.set_pin_heading(h);
            st.set_pin_masked(masked);
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
            .map(|(k, t, s)| ChoiceRow { key: k.into(), title: t.into(), subtitle: s.into() })
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
        cardreader::list_readers().map(|r| !r.is_empty()).unwrap_or(false)
    }
}

impl Interaction for Bridge {
    fn pin(&self, prompt: &str) -> Option<String> {
        if let Some(p) = self.pin_cache.lock().unwrap().clone() {
            return Some(p);
        }
        let p = self.ask_pin(prompt)?;
        *self.pin_cache.lock().unwrap() = Some(p.clone());
        Some(p)
    }

    fn choose_tan_method(&self, methods: &[TanMethod]) -> Option<String> {
        let rows = methods
            .iter()
            .map(|m| {
                let kind = match m.kind() {
                    TanKind::ChipTan => "chipTAN (optisch / USB)",
                    TanKind::Matrix => "photoTAN / QR-TAN",
                    TanKind::Decoupled => "Freigabe in der Banking-App",
                    TanKind::Sms => "mobileTAN / SMS",
                    TanKind::Other => "TAN-Verfahren",
                };
                (m.security_function.clone(), m.name.clone(), format!("{kind} · Verfahren {}", m.security_function))
            })
            .collect();
        self.choose("TAN-Verfahren wählen", rows)
    }

    fn choose_tan_medium(&self, media: &[String]) -> Option<String> {
        if media.is_empty() {
            return None;
        }
        let rows = media.iter().map(|m| (m.clone(), m.clone(), String::new())).collect();
        self.choose("TAN-Medium wählen", rows)
    }

    fn tan(&self, ch: &TanChallenge) -> TanAnswer {
        let text = fints::client::strip_html(&ch.text);
        let hhduc_text = ch.hhduc.as_ref().map(|b| fints::syntax::latin1_to_string(b)).unwrap_or_default();
        let flicker = FlickerCode::parse(&hhduc_text).or_else(|| FlickerCode::parse(&ch.text));
        let matrix = ch.hhduc.as_ref().and_then(|b| parse_matrix_code(b));
        let kind = if ch.decoupled {
            3
        } else if matrix.is_some() {
            2
        } else if flicker.is_some() && ch.method.kind() != TanKind::Matrix {
            1
        } else {
            0
        };
        let frames = if kind == 1 { flicker.as_ref().map(|f| f.frames()).unwrap_or_default() } else { vec![] };
        let usb = if kind == 1 { flicker.as_ref().map(|f| f.usb_payload()) } else { None };
        let usb_possible = usb.is_some() && self.has_reader();
        *self.usb_payload.lock().unwrap() = usb;
        let pixels = matrix.and_then(|(_, img)| image::load_from_memory(&img).ok()).map(|img| {
            let rgba = img.to_rgba8();
            SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(rgba.as_raw(), rgba.width(), rgba.height())
        });
        let title = SharedString::from(format!("Freigabe · {}", ch.method.name));
        let text = SharedString::from(text);
        let answer = self.ask(move |app| {
            let st = app.global::<AppState>();
            st.set_tan_title(title);
            st.set_tan_text(text);
            st.set_tan_kind(kind);
            st.set_tan_value(SharedString::default());
            st.set_tan_usb_possible(usb_possible);
            if let Some(p) = pixels {
                st.set_tan_image(slint::Image::from_rgba8(p));
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

    fn progress(&self, text: &str) {
        self.status(text);
    }

    fn text(&self, title: &str, prompt: &str) -> Option<String> {
        self.ask_input(title, prompt, false)
    }
}
