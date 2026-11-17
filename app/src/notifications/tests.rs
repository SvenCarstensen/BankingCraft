use super::*;
use crate::{bridge::Bridge, history::tests::Fixture, AppState, AppWindow};
use banking::Interaction;
use std::sync::atomic::Ordering;
fn take() -> Vec<Event> {
    EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
}
fn finish(f: &Fixture) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while f.ctx.running.load(Ordering::Acquire) && std::time::Instant::now() < until {
        f.app.pump(&egui::Context::default());
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(!f.ctx.running.load(Ordering::Acquire));
}
#[test]
fn reader_notifies_once_per_prompt_not_per_countdown_and_ignores_old_sessions() {
    let app = AppWindow::new().unwrap();
    let b = Bridge::new(app.as_weak());
    take();
    for remaining in (1..=30).rev() {
        b.reader_pin(remaining);
    }
    app.pump(&egui::Context::default());
    assert_eq!(take(), vec![Event::ReaderPin]);
    b.hardware_prompt(false, "");
    b.reader_pin(30);
    app.pump(&egui::Context::default());
    assert_eq!(take(), vec![Event::ClearReader, Event::ReaderPin]);
    b.hardware_prompt(true, "synthetic sensitive bank text");
    b.hardware_prompt(true, "synthetic sensitive bank text");
    app.pump(&egui::Context::default());
    assert_eq!(take(), vec![Event::ReaderConfirm]);
    b.hardware_prompt(false, "");
    app.pump(&egui::Context::default());
    take();
    b.reader_pin(30);
    b.generation.fetch_add(1, Ordering::AcqRel);
    app.pump(&egui::Context::default());
    assert!(take().is_empty());
    assert!(!app.global::<AppState>().get_show_pinpad());
    b.locked.store(true, Ordering::Release);
    b.reader_pin(30);
    app.pump(&egui::Context::default());
    assert!(take().is_empty());
}
#[test]
fn synchronization_signals_one_final_outcome_after_all_work_and_saving() {
    let f = Fixture::new();
    for outcome in [Outcome::Success, Outcome::Error, Outcome::Cancelled] {
        take();
        f.ctx.run_sync("Konten synchronisieren", move |c| {
            if outcome == Outcome::Cancelled {
                c.bridge.cancelled.store(true, Ordering::Release);
            }
            if outcome == Outcome::Success {
                Ok("synthetic private bank reply".into())
            } else {
                Err("synthetic private bank reply".into())
            }
        });
        // The synchronous reservation rejects a second trigger and its notification.
        f.ctx
            .run_sync("Konten synchronisieren", |_| panic!("duplicate sync"));
        finish(&f);
        assert_eq!(take(), vec![Event::ClearReader, Event::Sync(outcome)]);
    }
    take();
    f.ctx
        .run("Überweisung wird übermittelt …", |_| Ok("done".into()));
    finish(&f);
    assert_eq!(
        take(),
        vec![Event::ClearReader],
        "ordinary jobs must not claim to be a sync"
    );
}
#[test]
fn save_failure_is_an_error_and_locked_completion_still_signals_the_end() {
    let f = Fixture::new();
    take();
    let path = f.path.clone();
    let backup = f.dir.join("backup");
    f.ctx.run_sync("Konten synchronisieren", move |_| {
        std::fs::rename(&path, backup).unwrap();
        std::fs::create_dir(path).unwrap();
        Ok("bank read complete".into())
    });
    finish(&f);
    assert_eq!(
        take(),
        vec![Event::ClearReader, Event::Sync(Outcome::Error)]
    );
    std::fs::remove_dir(&f.path).unwrap();
    std::fs::rename(f.dir.join("backup"), &f.path).unwrap();
    f.ctx.run_sync("Konten synchronisieren", move |c| {
        c.bridge.cancelled.store(true, Ordering::Release);
        c.bridge.locked.store(true, Ordering::Release);
        Err("cancelled".into())
    });
    finish(&f);
    let events = take();
    assert_eq!(
        events
            .iter()
            .filter(|e| **e == Event::Sync(Outcome::Cancelled))
            .count(),
        1
    );
    assert!(f.ctx.file.lock().unwrap().is_none());
}
#[test]
fn notifications_only_use_localized_fixed_text_and_bundled_quiet_audio() {
    for language in [
        i18n::Language::German,
        i18n::Language::English,
        i18n::Language::French,
        i18n::Language::Spanish,
    ] {
        i18n::with_language(language, || {
            for event in [
                Event::ReaderPin,
                Event::ReaderConfirm,
                Event::Sync(Outcome::Success),
                Event::Sync(Outcome::Error),
                Event::Sync(Outcome::Cancelled),
            ] {
                let (title, body) = event.text();
                assert!(!title.is_empty() && !body.is_empty());
                assert!(!body.contains("synthetic private"));
            }
        });
    }
    let wav = include_bytes!("../../../macos/bankingcraft-gentle.wav");
    assert_eq!(&wav[..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
    let rate = u32::from_le_bytes(wav[24..28].try_into().unwrap());
    assert_eq!(rate, 44100);
    assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), 1);
    assert_eq!(u16::from_le_bytes(wav[34..36].try_into().unwrap()), 16);
    let samples: Vec<_> = wav[44..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|s| i16::from_le_bytes(*s) as i32)
        .collect();
    assert_eq!(samples.len(), 35280);
    assert!(samples.iter().map(|v| v.abs()).max().unwrap() < 3277);
    assert!(samples.iter().any(|v| *v != 0));
    assert_eq!(samples[0], 0);
    assert_eq!(*samples.last().unwrap(), 0);
}
