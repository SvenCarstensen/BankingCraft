//! Appearance is available before the vault is unlocked and contains no bank data.
use serde::{Deserialize, Serialize};

use std::{cell::Cell, io::Read, path::Path, rc::Rc};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    #[default]
    System,
    Light,
    Dark,
}

impl Mode {
    fn from_index(index: i32) -> Option<Self> {
        match index { 0 => Some(Self::System), 1 => Some(Self::Light), 2 => Some(Self::Dark), _ => None }
    }

    fn index(self) -> i32 {
        match self { Self::System => 0, Self::Light => 1, Self::Dark => 2 }
    }


}

#[derive(Default, Serialize, Deserialize)]
struct Preferences {
    #[serde(default)]
    theme: Mode,
}

fn load(path: &Path) -> Mode {
    let read = || -> Option<Preferences> {
        let file = std::fs::File::open(path).ok()?;
        if file.metadata().ok()?.len() > 4096 { return None; }
        let mut data = Vec::new();
        file.take(4097).read_to_end(&mut data).ok()?;
        if data.len() > 4096 { return None; }
        serde_json::from_slice(&data).ok()
    };
    read().unwrap_or_default().theme
}

fn save(path: &Path, theme: Mode) -> std::io::Result<()> {
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    crate::private_file::replace(path, &serde_json::to_vec(&Preferences { theme })?)
}

fn apply(app: &crate::AppWindow, mode: Mode) {
    app.global::<crate::AppState>().set_theme_mode(mode.index());
}

pub(crate) fn install(app: &crate::AppWindow) {
    let path = crate::store::default_path().with_file_name("appearance.json");
    // No preference means follow the system, including subsequent system changes.
    let current = Rc::new(Cell::new(load(&path)));
    apply(app, current.get());
    let weak = app.as_weak();
    app.global::<crate::AppState>().on_change_theme(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let Some(mode) = Mode::from_index(index) else { apply(&app, current.get()); return };
        if let Err(e) = save(&path, mode) {
            apply(&app, current.get());
            crate::toast(&app, &i18n::tr_format!("Design konnte nicht gespeichert werden: {e}"), true);
            return;
        }
        current.set(mode);
        crate::history::record(&app,"Design geändert",crate::history::Status::Success);
        apply(&app, mode);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("bankingcraft-appearance-{:016x}", rand::random::<u64>()));
        std::fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn first_start_and_invalid_preferences_follow_the_system() {
        let dir = directory(); let path = dir.join("appearance.json");
        assert_eq!(load(&path), Mode::System);
        for data in ["{}", "broken", "{\"theme\":\"future-value\"}"] {
            std::fs::write(&path, data).unwrap();
            assert_eq!(load(&path), Mode::System);
        }
        std::fs::write(&path, format!("{{\"theme\":\"dark\"}}{}", " ".repeat(4096))).unwrap();
        assert_eq!(load(&path), Mode::System);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn switching_and_restarting_preserves_each_choice() {
        let dir = directory(); let path = dir.join("preferences/appearance.json");
        for mode in [Mode::Dark, Mode::Light, Mode::System, Mode::Dark] {
            save(&path, mode).unwrap();
            assert_eq!(load(&path), mode);
            assert_eq!(Mode::from_index(mode.index()), Some(mode));
        }
        assert!(Mode::from_index(-1).is_none());
        assert!(Mode::from_index(3).is_none());
        // A failed write must be reported instead of pretending the choice was saved.
        assert!(save(&path.join("invalid-child"), Mode::Light).is_err());
        assert_eq!(load(&path), Mode::Dark);
        std::fs::remove_dir_all(dir).unwrap();
    }

}
