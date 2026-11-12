//! Window preferences are independent of the encrypted vault (also work while locked).
use serde::{Deserialize, Serialize};
use slint::{
    winit_030::{winit, EventResult, WinitWindowAccessor},
    ComponentHandle,
};
use std::{
    cell::{Cell, RefCell},
    io::Read,
    path::{Path, PathBuf},
    rc::Rc,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Geometry {
    x: i32,
    y: i32,
    // Logical content size, physical outer position (matches the native window APIs).
    width: f64,
    height: f64,
    maximized: bool,
    fullscreen: bool,
}

#[derive(Clone, Copy)]
struct Screen {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    scale: f64,
}

impl Geometry {
    fn valid(&self) -> bool {
        (1100.0..=16_000.0).contains(&self.width)
            && (680.0..=16_000.0).contains(&self.height)
            && i64::from(self.x).abs() <= 10_000_000
            && i64::from(self.y).abs() <= 10_000_000
    }

    fn fit(mut self, screens: &[Screen]) -> Self {
        // Keep the title bar reachable, including when the previous screen was unplugged.
        let reachable = screens.iter().any(|s| {
            let x = i64::from(self.x);
            let y = i64::from(self.y);
            x + (self.width * s.scale) as i64 >= i64::from(s.x) + 100
                && x + 100 <= i64::from(s.x) + i64::from(s.width)
                && y >= i64::from(s.y)
                && y + 40 <= i64::from(s.y) + i64::from(s.height)
        });
        // Preserve the user's exact placement, even if the bottom/right edge extends
        // beyond a screen. Only relocate a window whose title bar is unreachable.
        if reachable {
            return self;
        }
        let Some(s) = screens.first() else {
            return self;
        };
        self.width = self.width.min((f64::from(s.width) / s.scale).max(1100.0));
        self.height = self
            .height
            .min((f64::from(s.height) / s.scale - 32.0).max(680.0));
        let right = i64::from(s.x) + i64::from(s.width) - (self.width * s.scale) as i64;
        let bottom = i64::from(s.y) + i64::from(s.height) - ((self.height + 28.0) * s.scale) as i64;
        self.x = i64::from(self.x).clamp(i64::from(s.x), right.max(i64::from(s.x))) as i32;
        self.y = i64::from(self.y).clamp(i64::from(s.y), bottom.max(i64::from(s.y))) as i32;
        self
    }
}

fn load(path: &Path) -> Option<Geometry> {
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > 4096 {
        return None;
    }
    let mut data = Vec::new();
    file.take(4097).read_to_end(&mut data).ok()?;
    if data.len() > 4096 {
        return None;
    }
    let geometry: Geometry = serde_json::from_slice(&data).ok()?;
    geometry.valid().then_some(geometry)
}

#[derive(Clone)]
pub(crate) struct WindowState {
    path: PathBuf,
    geometry: Rc<RefCell<Option<Geometry>>>,
    ready: Rc<Cell<bool>>,
    capture_generation: Rc<Cell<u64>>,
    app: Rc<RefCell<Option<slint::Weak<crate::AppWindow>>>>,
}

impl WindowState {
    pub(crate) fn new() -> Self {
        let path = crate::store::default_path().with_file_name("window-state.json");
        let saved = load(&path);
        Self {
            path,
            geometry: Rc::new(RefCell::new(saved)),
            ready: Rc::new(Cell::new(false)),
            capture_generation: Rc::new(Cell::new(0)),
            app: Rc::new(RefCell::new(None)),
        }
    }

    pub(crate) fn save(&self) {
        let geometry = self.geometry.borrow();
        let Some(geometry) = geometry.as_ref().filter(|g| g.valid()) else {
            return;
        };
        let result = (|| {
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            crate::private_file::replace(&self.path, &serde_json::to_vec(geometry)?)
        })();
        if result.is_err() {
            log::warn!("Fenstereinstellungen konnten nicht gespeichert werden");
        }
    }

    fn capture(&self, window: &slint::Window) {
        if !self.ready.get() {
            return;
        }
        window.with_winit_window(|native| {
            if native.is_minimized() == Some(true) {
                return;
            }
            let maximized = native.is_maximized();
            let fullscreen = native.fullscreen().is_some();
            let mut state = self.geometry.borrow_mut();
            // Preserve normal geometry while zoomed/fullscreen; never persist minimized size.
            if maximized || fullscreen {
                if let Some(g) = state.as_mut() {
                    g.maximized = maximized;
                    g.fullscreen = fullscreen;
                }
            } else if let Ok(position) = native.outer_position() {
                let size = native.inner_size().to_logical::<f64>(native.scale_factor());
                let geometry = Geometry {
                    x: position.x,
                    y: position.y,
                    width: size.width,
                    height: size.height,
                    maximized,
                    fullscreen,
                };
                if geometry.valid() {
                    *state = Some(geometry);
                }
            }
        });
    }

    fn queue_capture(&self) {
        // macOS reports intermediate sizes during the zoom animation before
        // is_maximized becomes true. Do not replace normal geometry with them.
        let generation = self.capture_generation.get().wrapping_add(1);
        self.capture_generation.set(generation);
        let tracked = self.clone();
        slint::Timer::single_shot(std::time::Duration::from_millis(300), move || {
            if tracked.capture_generation.get() == generation {
                tracked.capture_current();
            }
        });
    }

    fn capture_current(&self) {
        if let Some(app) = self.app.borrow().as_ref().and_then(slint::Weak::upgrade) {
            self.capture(app.window());
        }
    }
}

// macOS Quit (Cmd-Q) terminates NSApplication without returning from app.run().
// Winit still dispatches LoopExiting, so save here as well as on window close.
impl slint::winit_030::CustomApplicationHandler for WindowState {
    fn exiting(&mut self, _: &winit::event_loop::ActiveEventLoop) -> EventResult {
        self.capture_current();
        self.save();
        EventResult::Propagate
    }
}

impl WindowState {
    pub(crate) fn install(&self, app: &crate::AppWindow) {
        *self.app.borrow_mut() = Some(app.as_weak());
        let saved = self.geometry.borrow().clone();
        if let Some(g) = &saved {
            app.window()
                .set_size(slint::LogicalSize::new(g.width as f32, g.height as f32));
            app.window()
                .set_position(slint::PhysicalPosition::new(g.x, g.y));
        }
        let tracked = self.clone();
        app.window().on_winit_window_event(move |window, event| {
            if matches!(
                event,
                winit::event::WindowEvent::Moved(_)
                    | winit::event::WindowEvent::Resized(_)
                    | winit::event::WindowEvent::Focused(_)
            ) {
                tracked.queue_capture();
            } else if matches!(
                event,
                winit::event::WindowEvent::CloseRequested | winit::event::WindowEvent::Destroyed
            ) {
                tracked.capture(window);
                tracked.save();
            } else if matches!(event, winit::event::WindowEvent::RedrawRequested)
                && tracked.geometry.borrow().is_none()
            {
                tracked.capture(window);
            }
            EventResult::Propagate
        });
        let tracked = self.clone();
        let weak = app.as_weak();
        app.window().on_close_requested(move || {
            if let Some(app) = weak.upgrade() {
                tracked.capture(app.window());
            }
            tracked.save();
            slint::CloseRequestResponse::HideWindow
        });
        // The native monitor list becomes available once the event loop has started.
        let tracked = self.clone();
        let weak = app.as_weak();
        slint::Timer::single_shot(std::time::Duration::ZERO, move || {
            let Some(app) = weak.upgrade() else { return };
            if let Some(saved) = saved {
                let screens = app
                    .window()
                    .with_winit_window(|native| {
                        // Current screen first is the fallback if the old display no longer exists.
                        let monitors = native
                            .current_monitor()
                            .into_iter()
                            .chain(native.available_monitors());
                        monitors
                            .map(|m| Screen {
                                x: m.position().x,
                                y: m.position().y,
                                width: m.size().width,
                                height: m.size().height,
                                scale: m.scale_factor(),
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let saved = saved.fit(&screens);
                app.window().set_size(slint::LogicalSize::new(
                    saved.width as f32,
                    saved.height as f32,
                ));
                app.window()
                    .set_position(slint::PhysicalPosition::new(saved.x, saved.y));
                app.window().set_maximized(saved.maximized);
                app.window().set_fullscreen(saved.fullscreen);
                *tracked.geometry.borrow_mut() = Some(saved);
            }
            tracked.ready.set(true);
            tracked.queue_capture();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn normal() -> Geometry {
        Geometry {
            x: 120,
            y: 100,
            width: 1200.0,
            height: 740.0,
            maximized: false,
            fullscreen: false,
        }
    }
    #[test]
    fn normal_position_and_logical_size_survive_save_and_restart() {
        let path = std::env::temp_dir().join(format!(
            "bankingcraft-window-{:016x}.json",
            rand::random::<u64>()
        ));
        let state = WindowState {
            path: path.clone(),
            geometry: Rc::new(RefCell::new(Some(normal()))),
            ready: Rc::new(Cell::new(true)),
            capture_generation: Rc::new(Cell::new(0)),
            app: Rc::new(RefCell::new(None)),
        };
        state.save();
        assert_eq!(load(&path), Some(normal()));
        let mut changed = normal();
        changed.x = -1600;
        changed.maximized = true;
        *state.geometry.borrow_mut() = Some(changed.clone());
        state.save();
        assert_eq!(load(&path), Some(changed));
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn missing_or_corrupt_preferences_use_default_window() {
        let path = std::env::temp_dir().join(format!(
            "bankingcraft-window-{:016x}.json",
            rand::random::<u64>()
        ));
        assert!(load(&path).is_none());
        for data in [
            b"{".to_vec(),
            vec![b' '; 4097],
            serde_json::to_vec(&Geometry {
                width: 0.0,
                ..normal()
            })
            .unwrap(),
        ] {
            std::fs::write(&path, data).unwrap();
            assert!(load(&path).is_none());
        }
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn restores_negative_coordinates_on_connected_monitor() {
        let screens = [
            Screen {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
                scale: 1.0,
            },
            Screen {
                x: -1920,
                y: 0,
                width: 1920,
                height: 1080,
                scale: 1.0,
            },
        ];
        let g = Geometry {
            x: -1800,
            ..normal()
        };
        assert_eq!(g.clone().fit(&screens), g);
        let retina = Screen {
            x: 0,
            y: 0,
            width: 3840,
            height: 2160,
            scale: 2.0,
        };
        assert_eq!(normal().fit(&[retina]), normal());
    }
    #[test]
    fn disconnected_monitor_and_larger_old_window_are_recovered() {
        let screen = Screen {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            scale: 1.0,
        };
        let recovered = Geometry {
            x: -5000,
            y: 9000,
            width: 4000.0,
            height: 3000.0,
            ..normal()
        }
        .fit(&[screen]);
        assert_eq!(recovered.x, 0);
        assert!(recovered.y >= 0 && recovered.y < 100);
        assert_eq!(recovered.width, 1920.0);
        assert_eq!(recovered.height, 1048.0);
        assert_eq!(normal().fit(&[]), normal());
    }

    #[test]
    fn reachable_window_keeps_exact_position_near_screen_edges() {
        let screen = Screen {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            scale: 1.0,
        };
        let g = Geometry {
            x: -100,
            y: 500,
            ..normal()
        };
        assert_eq!(g.clone().fit(&[screen]), g);
    }
}
