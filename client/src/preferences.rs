//! Client-only preferences. Never part of a connection or kit.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Reload {
    R,
    LeftBracket,
    RightBracket,
}

impl Reload {
    pub(crate) fn key(self) -> KeyCode {
        match self {
            Self::R => KeyCode::KeyR,
            Self::LeftBracket => KeyCode::BracketLeft,
            Self::RightBracket => KeyCode::BracketRight,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::R => "R",
            Self::LeftBracket => "[",
            Self::RightBracket => "]",
        }
    }

    // Deliberate allowlist of unassigned gameplay keys, plus the default.
    // In particular, menu navigation, letters used by other actions and
    // the letter-only prone configuration cannot collide with these choices.
    pub(crate) fn next(self) -> Self {
        match self {
            Self::R => Self::LeftBracket,
            Self::LeftBracket => Self::RightBracket,
            Self::RightBracket => Self::R,
        }
    }
}

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Preferences {
    pub fov: u16,
    pub sensitivity_percent: u16,
    pub reload: Reload,
    pub ui_scale_percent: u16,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            fov: 70,
            sensitivity_percent: 100,
            reload: Reload::R,
            ui_scale_percent: 100,
        }
    }
}

impl Preferences {
    pub(crate) fn sensitivity(self) -> f32 {
        f32::from(self.sensitivity_percent) / 100.0
    }

    pub(crate) fn ui_scale(self) -> f32 {
        f32::from(self.ui_scale_percent) / 100.0
    }

    fn valid(self) -> bool {
        (20..=160).contains(&self.fov)
            && (25..=300).contains(&self.sensitivity_percent)
            && matches!(self.ui_scale_percent, 100 | 125)
    }
}

#[derive(Resource, Default)]
pub(crate) struct Store {
    path: Option<PathBuf>,
    pub(crate) load_warning: bool,
}

impl Store {
    #[cfg(test)]
    pub(crate) fn test_path(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            load_warning: false,
        }
    }

    pub(crate) fn open() -> (Self, Preferences) {
        let path = std::env::var_os("TK_PREFERENCES_PATH")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("XDG_CONFIG_HOME")
                    .filter(|p| !p.is_empty())
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
                    .map(|p| p.join("tk").join("preferences.json"))
            });
        let (prefs, load_warning) = path
            .as_deref()
            .map_or((Preferences::default(), false), Self::load);
        (Self { path, load_warning }, prefs)
    }

    fn load(path: &Path) -> (Preferences, bool) {
        match fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<Preferences>(&bytes) {
                Ok(prefs) if prefs.valid() => (prefs, false),
                _ => (Preferences::default(), true),
            },
            Err(error) => (
                Preferences::default(),
                error.kind() != io::ErrorKind::NotFound,
            ),
        }
    }

    pub(crate) fn save(&self, prefs: Preferences) -> io::Result<()> {
        let Some(path) = self
            .path
            .as_deref()
            .filter(|path| prefs.valid() && path.file_name().is_some())
        else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "missing preference path or invalid values",
            ));
        };
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let temp = parent.join(format!(
            ".tk-preferences-{:016x}.tmp",
            rand::random::<u64>()
        ));
        use io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        let result = (|| {
            serde_json::to_writer_pretty(&mut file, &prefs)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_persist_without_accepting_conflicting_keys_or_bad_numbers() {
        let dir =
            std::env::temp_dir().join(format!("tk-prefs-test-{:016x}", rand::random::<u64>()));
        let path = dir.join("preferences.json");
        let store = Store {
            path: Some(path.clone()),
            load_warning: false,
        };
        let prefs = Preferences {
            fov: 90,
            sensitivity_percent: 125,
            reload: Reload::LeftBracket,
            ui_scale_percent: 125,
        };
        assert_eq!(Store::load(&path), (Preferences::default(), false));
        store.save(prefs).unwrap();
        assert_eq!(Store::load(&path), (prefs, false));
        assert!(store.save(Preferences { fov: 0, ..prefs }).is_err());
        assert_eq!(Store::load(&path), (prefs, false));
        for invalid in [
            r#"{"reload":"Enter"}"#,
            r#"{"reload":"KeyF"}"#,
            r#"{"sensitivity_percent":301}"#,
            r#"{"ui_scale_percent":200}"#,
            r#"{"fov":null}"#,
        ] {
            fs::write(&path, invalid).unwrap();
            assert_eq!(Store::load(&path), (Preferences::default(), true));
        }
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
