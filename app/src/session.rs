//! Session expiry is independent of bank progress. Lock immediately hides all
//! data and revokes interactive callbacks; a running job is cancelled and gets
//! one chance to persist its final (possibly uncertain) result before key drop.
use crate::{AppState, AppWindow};
use std::{sync::atomic::Ordering, time::{Duration, Instant, SystemTime}};

const IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const RESUME_GAP: Duration = Duration::from_secs(15);
static SYSTEM_LOCK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static MONITOR_AVAILABLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static MONITOR_STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn expire_from_system() { SYSTEM_LOCK.store(true, Ordering::Release); }
pub fn request_lock() { expire_from_system(); }

pub fn start_monitor() {
    static START: std::sync::Once = std::sync::Once::new();
    START.call_once(|| {
        MONITOR_STARTED.store(true, Ordering::Release);
        #[cfg(target_os = "macos")] {
            unsafe extern "C" { fn bankingcraft_observe_session(expire: extern "C" fn()); }
            // Installed on the main UI thread; observers call only an atomic flag.
            unsafe { bankingcraft_observe_session(expire_from_system); }
            MONITOR_AVAILABLE.store(true, Ordering::Release);
        }
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        std::thread::spawn(|| loop {
            if let Some(locked) = system_locked() {
                MONITOR_AVAILABLE.store(true, Ordering::Release);
                if locked { expire_from_system(); }
            } else { MONITOR_AVAILABLE.store(false, Ordering::Release); }
            std::thread::sleep(Duration::from_secs(1));
        });
    });
}

pub fn take_system_lock(focused: bool) -> bool {
    SYSTEM_LOCK.swap(false, Ordering::AcqRel) || (MONITOR_STARTED.load(Ordering::Acquire) && !focused && !MONITOR_AVAILABLE.load(Ordering::Acquire))
}

#[cfg(target_os = "windows")]
fn system_locked() -> Option<bool> {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn OpenInputDesktop(flags: u32, inherit: i32, access: u32) -> *mut std::ffi::c_void;
        fn CloseDesktop(desktop: *mut std::ffi::c_void) -> i32;
    }
    // The secure lock/UAC desktop is inaccessible from the user desktop. This
    // conservatively locks for both; it never treats focus changes as unlocks.
    let desktop = unsafe { OpenInputDesktop(0, 0, 1) };
    if desktop.is_null() { Some(true) }
    else { unsafe { CloseDesktop(desktop); } Some(false) }
}

#[cfg(target_os = "linux")]
fn system_locked() -> Option<bool> {
    use zbus::blocking::{Connection, Proxy};
    let mut known = None;
    if let Ok(bus) = Connection::system() {
        if let Ok(manager) = Proxy::new(&bus,"org.freedesktop.login1","/org/freedesktop/login1","org.freedesktop.login1.Manager") {
            if let Ok(path) = manager.call::<_,_,zbus::zvariant::OwnedObjectPath>("GetSessionByPID",&(std::process::id(),)) {
                if let Ok(session) = Proxy::new(&bus,"org.freedesktop.login1",path,"org.freedesktop.login1.Session") {
                    if let Ok(value) = session.get_property::<bool>("LockedHint") {
                        if value { return Some(true); } known=Some(false);
                    }
                }
            }
        }
    }
    if let Ok(bus) = Connection::session() {
        for (name,path) in [("org.freedesktop.ScreenSaver","/org/freedesktop/ScreenSaver"),("org.gnome.ScreenSaver","/org/gnome/ScreenSaver")] {
            if let Ok(proxy) = Proxy::new(&bus,name,path,name) {
                if let Ok(value) = proxy.call::<_,_,bool>("GetActive",&()) {
                    if value { return Some(true); } known=Some(false);
                }
            }
        }
    }
    known
}

pub struct Guard {
    last_input: Instant,
    last_tick: Instant,
    last_wall: SystemTime,
    was_unlocked: bool,
}
impl Default for Guard {
    fn default() -> Self {
        Self { last_input: Instant::now(), last_tick: Instant::now(), last_wall: SystemTime::now(), was_unlocked: false }
    }
}
impl Guard {
    pub fn tick(&mut self, unlocked: bool, input: bool, system_locked: bool, now: Instant, wall: SystemTime) -> bool {
        let resumed = now.saturating_duration_since(self.last_tick) > RESUME_GAP
            || wall.duration_since(self.last_wall).is_ok_and(|d| d > RESUME_GAP);
        self.last_tick = now;
        self.last_wall = wall;
        let expired = unlocked && (system_locked || (self.was_unlocked
            && (resumed || now.saturating_duration_since(self.last_input) >= IDLE_TIMEOUT)));
        // Evaluate expiry before new input: the first key after wake must not
        // reset the clock before we have locked the previous session.
        if !self.was_unlocked || input { self.last_input = now; }
        self.was_unlocked = unlocked && !expired;
        expired
    }
}

pub fn lock(app: &AppWindow) {
    let Some(c) = crate::CTX.with(|c| c.borrow().clone()) else { return };
    if c.bridge.locked.swap(true, Ordering::AcqRel) { return; }
    c.vault.lock().unwrap().history.add("Tresor gesperrt",crate::history::Status::Success,None);
    c.bridge.generation.fetch_add(1, Ordering::AcqRel);
    c.bridge.cancelled.store(true, Ordering::Release);
    c.bridge.answer(crate::bridge::Answer::Cancel);
    let st = app.global::<AppState>();
    crate::vaults::reset_vault_view(app);
    st.set_unlocked(false);
    st.set_show_vault_manager(false);
    st.set_active_vault_name(Default::default());
    st.set_busy(false);
    st.set_vault_selection_revision(st.get_vault_selection_revision().wrapping_add(1));
    if !c.running.load(Ordering::Acquire) {
        if let Err(e) = c.save() {
            // Preserve on-disk data and erase the session even if disk is full.
            st.set_unlock_error(format!("Tresor gesperrt. Letztes Speichern fehlgeschlagen: {e}"));
        }
        finish_lock(app);
    } else {
        st.set_unlock_error("Tresor gesperrt. Laufender Bankvorgang wird beendet …".into());
    }
}

pub fn finish_lock(app: &AppWindow) {
    let Some(c) = crate::CTX.with(|c| c.borrow().clone()) else { return };
    if !c.bridge.locked.load(Ordering::Acquire) || c.running.load(Ordering::Acquire) { return; }
    *c.bridge.pin_cache.lock().unwrap() = None;
    *c.bridge.usb_payload.lock().unwrap() = None;
    *c.vault.lock().unwrap() = Default::default();
    // Zeroizing key and process lock are released together.
    *c.file.lock().unwrap() = None;
    crate::vaults::reset_vault_view(app);
    let st = app.global::<AppState>();
    st.set_busy(false);
    if st.get_unlock_error() == "Tresor gesperrt. Laufender Bankvorgang wird beendet …" {
        st.set_unlock_error(Default::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lock_revokes_ui_and_releases_key_after_worker_finishes() {
        use crate::{bridge::Bridge, jobs::Ctx, store::{Vault, VaultFile}};
        use std::sync::{Arc, Mutex, atomic::AtomicBool};
        for running in [false, true] {
            let directory = std::env::temp_dir().join(format!("bankingcraft-lock-test-{}", rand::random::<u64>()));
            std::fs::create_dir(&directory).unwrap();
            let path = directory.join("test.kontor");
            let app = AppWindow::new().unwrap();
            let mut vault = Vault::default(); crate::demo::add_demo(&mut vault);
            let file = VaultFile::create_new(path.clone(), "synthetic-password", &vault).unwrap();
            let c = Ctx { vault: Arc::new(Mutex::new(vault)), file: Arc::new(Mutex::new(Some(file))),
                bridge: Bridge::new(app.as_weak()), running: Arc::new(AtomicBool::new(running)) };
            crate::CTX.with(|ctx| *ctx.borrow_mut() = Some(c.clone()));
            let st = app.global::<AppState>();
            st.set_unlocked(true); st.set_pin_value("synthetic-pin".into()); st.set_tan_value("synthetic-tan".into());
            st.set_tf_name("sensitive recipient".into()); st.set_show_pin(true);
            *c.bridge.pin_cache.lock().unwrap() = Some(zeroize::Zeroizing::new("synthetic-pin".into()));
            crate::CONFIRM.with(|callback| *callback.borrow_mut() = Some(Box::new(|| panic!("revoked confirmation"))));
            lock(&app);
            assert!(!st.get_unlocked() && !st.get_show_pin());
            assert!(st.get_pin_value().is_empty() && st.get_tan_value().is_empty() && st.get_tf_name().is_empty());
            assert!(c.bridge.pin_cache.lock().unwrap().is_none());
            assert!(crate::CONFIRM.with(|callback| callback.borrow().is_none()));
            assert_eq!(c.file.lock().unwrap().is_some(), running);
            if running { c.save().unwrap(); c.running.store(false, Ordering::Release); finish_lock(&app); }
            assert!(c.file.lock().unwrap().is_none() && c.vault.lock().unwrap().accounts.is_empty());
            assert!(st.get_unlock_error().is_empty());
            let reopened = VaultFile::open_session(path, "synthetic-password").unwrap();
            assert!(!reopened.1.accounts.is_empty()); drop(reopened);
            crate::CTX.with(|ctx| *ctx.borrow_mut() = None);
            std::fs::remove_dir_all(directory).unwrap();
        }
    }
    #[test]
    fn input_on_expired_session_does_not_prevent_lock() {
        let mut guard = Guard::default(); let now = Instant::now(); let wall = SystemTime::now();
        assert!(!guard.tick(true,true,false,now,wall));
        for second in 1..300 { assert!(!guard.tick(true,false,false,now+Duration::from_secs(second),wall+Duration::from_secs(second))); }
        assert!(guard.tick(true,true,false,now+IDLE_TIMEOUT,wall+IDLE_TIMEOUT));
    }
    #[test]
    fn screen_lock_and_both_suspend_clocks_expire_before_input() {
        for kind in 0..3 {
            let mut guard = Guard::default(); let now = Instant::now(); let wall = SystemTime::now();
            guard.tick(true,true,false,now,wall);
            let delta=RESUME_GAP+Duration::from_secs(1);
            assert!(guard.tick(true,true,kind==0,now+if kind==1 {delta} else {Duration::ZERO},wall+if kind==2 {delta} else {Duration::ZERO}));
        }
    }
}
