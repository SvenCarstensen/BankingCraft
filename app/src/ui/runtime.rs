use super::AppState;
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    marker::PhantomData,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
type Event = Box<dyn FnOnce(AppWindow) + Send>;
#[derive(Default)]
struct Dispatcher {
    queue: Mutex<VecDeque<Event>>,
    context: Mutex<Option<egui::Context>>,
}
struct Inner {
    id: usize,
    state: AppState,
    dispatcher: Arc<Dispatcher>,
    native: RefCell<Option<Arc<winit::window::Window>>>,
}
thread_local! {
    static WINDOWS: RefCell<HashMap<usize, std::rc::Weak<Inner>>> = RefCell::new(HashMap::new());
    static TIMERS: RefCell<Vec<Scheduled>> = const { RefCell::new(Vec::new()) };
}
#[derive(Clone)]
pub struct AppWindow(Rc<Inner>);
impl AppWindow {
    pub fn new() -> Result<Self, std::convert::Infallible> {
        static NEXT: AtomicUsize = AtomicUsize::new(1);
        let inner = Rc::new(Inner {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            state: AppState::default(),
            dispatcher: Arc::new(Dispatcher::default()),
            native: RefCell::new(None),
        });
        WINDOWS.with(|w| {
            let mut w = w.borrow_mut();
            w.retain(|_, v| v.strong_count() > 0);
            w.insert(inner.id, Rc::downgrade(&inner));
        });
        Ok(Self(inner))
    }
    pub fn global<T: From<AppState>>(&self) -> T {
        T::from(self.0.state.clone())
    }
    pub fn as_weak(&self) -> Weak<Self> {
        Weak {
            id: self.0.id,
            dispatcher: Arc::downgrade(&self.0.dispatcher),
            marker: PhantomData,
        }
    }
    pub fn attach(&self, ctx: &egui::Context) {
        *self.0.dispatcher.context.lock().unwrap() = Some(ctx.clone());
    }
    pub fn attach_native(&self, native: Arc<winit::window::Window>) {
        *self.0.native.borrow_mut() = Some(native);
    }
    pub fn with_native(&self, action: impl FnOnce(&winit::window::Window)) {
        if let Some(native) = self.0.native.borrow().as_ref() {
            action(native);
        }
    }
    pub fn pump(&self, ctx: &egui::Context) {
        let queue = std::mem::take(&mut *self.0.dispatcher.queue.lock().unwrap());
        for event in queue {
            event(self.clone());
        }
        // Remove due timers before calling user code, which may itself create timers.
        let now = Instant::now();
        let due = TIMERS.with(|timers| {
            let mut timers = timers.borrow_mut();
            let mut due = Vec::new();
            let mut i = 0;
            while i < timers.len() {
                if timers[i].cancelled() {
                    timers.remove(i);
                } else if timers[i].due <= now {
                    due.push(timers.remove(i));
                } else {
                    i += 1;
                }
            }
            due
        });
        for mut timer in due {
            (timer.action)();
            if let Some(interval) = timer.interval {
                timer.due = now + interval;
                if !timer.cancelled() {
                    TIMERS.with(|t| t.borrow_mut().push(timer));
                }
            }
        }
        if let Some(next) = TIMERS.with(|t| t.borrow().iter().map(|t| t.due).min()) {
            ctx.request_repaint_after(next.saturating_duration_since(Instant::now()));
        }
    }
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.dispatcher.context.lock().unwrap().take();
    }
}
/// Sendable handle. UI state can only be upgraded on its owning thread.
pub struct Weak<T> {
    id: usize,
    dispatcher: std::sync::Weak<Dispatcher>,
    marker: PhantomData<fn() -> T>,
}
impl<T> Clone for Weak<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            dispatcher: self.dispatcher.clone(),
            marker: PhantomData,
        }
    }
}
impl<T> Default for Weak<T> {
    fn default() -> Self {
        Self {
            id: 0,
            dispatcher: Default::default(),
            marker: PhantomData,
        }
    }
}
impl Weak<AppWindow> {
    pub fn upgrade(&self) -> Option<AppWindow> {
        WINDOWS.with(|w| {
            w.borrow()
                .get(&self.id)
                .and_then(std::rc::Weak::upgrade)
                .map(AppWindow)
        })
    }
    pub fn unwrap(&self) -> AppWindow {
        self.upgrade().expect("UI handle on its owning thread")
    }
    pub fn upgrade_in_event_loop(
        &self,
        f: impl FnOnce(AppWindow) + Send + 'static,
    ) -> Result<(), &'static str> {
        let dispatcher = self.dispatcher.upgrade().ok_or("window closed")?;
        dispatcher.queue.lock().unwrap().push_back(Box::new(f));
        if let Some(ctx) = dispatcher.context.lock().unwrap().as_ref() {
            ctx.request_repaint();
        }
        Ok(())
    }
}
struct Scheduled {
    due: Instant,
    interval: Option<Duration>,
    alive: Option<std::rc::Weak<Cell<bool>>>,
    action: Box<dyn FnMut()>,
}
impl Scheduled {
    fn cancelled(&self) -> bool {
        self.alive
            .as_ref()
            .is_some_and(|a| a.upgrade().is_none_or(|a| !a.get()))
    }
}
#[derive(Default)]
pub struct Timer {
    alive: RefCell<Option<Rc<Cell<bool>>>>,
}
fn wake_ui() {
    WINDOWS.with(|windows| {
        for inner in windows.borrow().values().filter_map(std::rc::Weak::upgrade) {
            if let Some(ctx) = inner.dispatcher.context.lock().unwrap().as_ref() {
                ctx.request_repaint();
            }
        }
    });
}
pub enum TimerMode {
    Repeated,
}
impl Timer {
    pub fn start(&self, _: TimerMode, interval: Duration, action: impl FnMut() + 'static) {
        wake_ui();
        let alive = Rc::new(Cell::new(true));
        *self.alive.borrow_mut() = Some(alive.clone());
        TIMERS.with(|t| {
            t.borrow_mut().push(Scheduled {
                due: Instant::now() + interval,
                interval: Some(interval),
                alive: Some(Rc::downgrade(&alive)),
                action: Box::new(action),
            })
        });
    }
    pub fn single_shot(duration: Duration, action: impl FnOnce() + 'static) {
        wake_ui();
        let mut action = Some(action);
        TIMERS.with(|t| {
            t.borrow_mut().push(Scheduled {
                due: Instant::now() + duration,
                interval: None,
                alive: None,
                action: Box::new(move || {
                    if let Some(f) = action.take() {
                        f();
                    }
                }),
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_updates_are_queued_and_only_applied_on_the_ui_thread() {
        let app = AppWindow::new().unwrap();
        let weak = app.as_weak();
        let ctx = egui::Context::default();
        app.attach(&ctx);
        std::thread::spawn(move || {
            assert!(weak.upgrade().is_none());
            weak.upgrade_in_event_loop(|app| {
                app.global::<AppState>()
                    .set_busy_text("worker update".into())
            })
            .unwrap();
        })
        .join()
        .unwrap();
        assert!(app.global::<AppState>().get_busy_text().is_empty());
        app.pump(&ctx);
        assert_eq!(app.global::<AppState>().get_busy_text(), "worker update");
        let weak = app.as_weak();
        drop(app);
        assert!(weak.upgrade().is_none());
        assert!(weak
            .upgrade_in_event_loop(|_| panic!("closed window"))
            .is_err());
    }
    #[test]
    fn callbacks_can_change_state_and_replace_callbacks_without_borrowing_panics() {
        let app = AppWindow::new().unwrap();
        let state = app.global::<AppState>();
        let target = state.clone();
        state.on_navigate(move |page| {
            target.set_page(page);
            target.on_navigate(|_| {});
        });
        state.invoke_navigate(4);
        assert_eq!(state.get_page(), 4);
        state.invoke_navigate(7);
        assert_eq!(state.get_page(), 4);
        let before = state.snapshot();
        let mut after = before.clone();
        after.tf_name = "draft".into();
        state.set_busy_text("concurrent result".into());
        state.patch(&before, &after);
        assert_eq!(state.get_tf_name(), "draft");
        assert_eq!(state.get_busy_text(), "concurrent result");
    }
    #[test]
    fn dropped_repeating_timers_stop_and_single_shots_can_schedule_more_work() {
        let app = AppWindow::new().unwrap();
        let ctx = egui::Context::default();
        let count = Rc::new(Cell::new(0));
        let timer = Timer::default();
        let calls = count.clone();
        timer.start(TimerMode::Repeated, Duration::ZERO, move || {
            calls.set(calls.get() + 1)
        });
        app.pump(&ctx);
        assert_eq!(count.get(), 1);
        drop(timer);
        app.pump(&ctx);
        assert_eq!(count.get(), 1);
        let calls = count.clone();
        Timer::single_shot(Duration::ZERO, move || {
            calls.set(calls.get() + 1);
            Timer::single_shot(Duration::ZERO, move || calls.set(calls.get() + 1));
        });
        app.pump(&ctx);
        assert_eq!(count.get(), 2);
        app.pump(&ctx);
        assert_eq!(count.get(), 3);
    }
}
