//! Window preferences are independent of the encrypted vault (also work while locked).
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
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
}
impl WindowState {
    pub(crate) fn new() -> Self {
        let path = crate::store::default_path().with_file_name("window-state.json");
        let geometry = Rc::new(RefCell::new(load(&path)));
        Self { path, geometry }
    }
    pub(crate) fn restore(&self, window: &winit::window::Window) {
        let screens: Vec<_> = window
            .available_monitors()
            .map(|m| {
                let p = m.position();
                let s = m.size();
                Screen {
                    x: p.x,
                    y: p.y,
                    width: s.width,
                    height: s.height,
                    scale: m.scale_factor(),
                }
            })
            .collect();
        let Some(g) = self.geometry.borrow().clone().map(|g| g.fit(&screens)) else {
            return;
        };
        let _ = window.request_inner_size(winit::dpi::LogicalSize::new(g.width, g.height));
        window.set_outer_position(winit::dpi::PhysicalPosition::new(g.x, g.y));
        window.set_maximized(g.maximized);
        if g.fullscreen {
            window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
        }
    }
    pub(crate) fn capture(&self, window: &winit::window::Window) {
        if window.is_minimized() == Some(true) {
            return;
        }
        let maximized = window.is_maximized();
        let fullscreen = window.fullscreen().is_some();
        let mut state = self.geometry.borrow_mut();
        if maximized || fullscreen {
            if let Some(g) = state.as_mut() {
                g.maximized = maximized;
                g.fullscreen = fullscreen;
            }
        } else if let Ok(p) = window.outer_position() {
            let size = window.inner_size().to_logical::<f64>(window.scale_factor());
            let g = Geometry {
                x: p.x,
                y: p.y,
                width: size.width,
                height: size.height,
                maximized,
                fullscreen,
            };
            if g.valid() {
                *state = Some(g);
            }
        }
    }
    pub(crate) fn save(&self) {
        let state = self.geometry.borrow();
        let Some(g) = state.as_ref().filter(|g| g.valid()) else {
            return;
        };
        let result = (|| {
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            crate::private_file::replace(&self.path, &serde_json::to_vec(g)?)
        })();
        if result.is_err() {
            log::warn!("Fenstereinstellungen konnten nicht gespeichert werden");
        }
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
