//! User settings, recent files and custom themes, kept in the app data folder (D-24). Nothing
//! here is ever written into a project folder or a model file (D-19).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How many recent models the launcher lists.
const RECENT: usize = 12;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// The NetLogo install the user chose. None: look in the usual places.
    pub netlogo_home: Option<PathBuf>,
    /// Most recent first.
    pub recent: Vec<PathBuf>,
    /// A built-in theme id (`light`, `dark`, ...), `custom:<name>`, or empty to follow the
    /// system's light or dark setting.
    pub theme: String,
    /// Interface font size in pixels; 0 means the default.
    pub font_size: u32,
    /// Code font size in pixels; 0 means the default.
    pub code_font_size: u32,
    /// Keys Logos doesn't know yet, kept when the file is rewritten.
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

impl Settings {
    pub fn add_recent(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(RECENT);
    }
}

/// Where settings live, and reading and writing them.
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> Store {
        Store { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn settings_path(&self) -> PathBuf {
        self.dir.join("settings.json")
    }

    pub fn themes_dir(&self) -> PathBuf {
        self.dir.join("themes")
    }

    /// The settings, or the defaults when the file is missing or unreadable. An unreadable
    /// file is kept as `settings.json.bad` so nothing the user set is lost silently.
    pub fn load(&self) -> Settings {
        let path = self.settings_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(s) => s,
                Err(_) => {
                    let _ = std::fs::rename(&path, path.with_extension("json.bad"));
                    Settings::default()
                }
            },
            Err(_) => Settings::default(),
        }
    }

    /// Writes the settings through a temporary file, so a crash never leaves half a file.
    pub fn save(&self, settings: &Settings) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let text = serde_json::to_string_pretty(settings).map_err(std::io::Error::other)?;
        write_atomically(&self.settings_path(), text.as_bytes())
    }

    /// Custom themes: (file stem, JSON text), sorted by name.
    pub fn custom_themes(&self) -> Vec<(String, String)> {
        let Ok(entries) = std::fs::read_dir(self.themes_dir()) else {
            return Vec::new();
        };
        let mut out: Vec<(String, String)> = entries
            .filter_map(Result::ok)
            .filter_map(|e| {
                let p = e.path();
                if p.extension()? != "json" {
                    return None;
                }
                let name = p.file_stem()?.to_string_lossy().into_owned();
                Some((name, std::fs::read_to_string(&p).ok()?))
            })
            .collect();
        out.sort();
        out
    }

    pub fn save_custom_theme(&self, name: &str, json: &str) -> Result<(), String> {
        if !valid_theme_name(name) {
            return Err(format!(
                "\"{name}\" can't be a theme name: use letters, digits, spaces, - and _"
            ));
        }
        serde_json::from_str::<serde_json::Value>(json)
            .map_err(|e| format!("not a theme file: {e}"))?;
        let dir = self.themes_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        write_atomically(&dir.join(format!("{name}.json")), json.as_bytes())
            .map_err(|e| e.to_string())
    }

    pub fn delete_custom_theme(&self, name: &str) -> Result<(), String> {
        if !valid_theme_name(name) {
            return Err(format!("no theme \"{name}\""));
        }
        std::fs::remove_file(self.themes_dir().join(format!("{name}.json")))
            .map_err(|e| e.to_string())
    }
}

fn valid_theme_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
}

pub fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_keep_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("logos"));
        assert_eq!(store.load(), Settings::default());
        std::fs::create_dir_all(store.dir()).unwrap();
        std::fs::write(
            store.settings_path(),
            r#"{"theme":"dark","future":{"a":1}}"#,
        )
        .unwrap();
        let mut s = store.load();
        assert_eq!(s.theme, "dark");
        s.add_recent(Path::new("/m/a.nlogox"));
        s.add_recent(Path::new("/m/b.nlogox"));
        s.add_recent(Path::new("/m/a.nlogox"));
        store.save(&s).unwrap();
        let back = store.load();
        assert_eq!(
            back.recent,
            vec![PathBuf::from("/m/a.nlogox"), PathBuf::from("/m/b.nlogox")]
        );
        assert_eq!(back.other["future"]["a"], 1);
    }

    #[test]
    fn unreadable_settings_are_kept_aside() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        std::fs::write(store.settings_path(), "{not json").unwrap();
        assert_eq!(store.load(), Settings::default());
        assert!(dir.path().join("settings.json.bad").is_file());
    }

    #[test]
    fn custom_themes() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        store
            .save_custom_theme("My theme", r#"{"base":"dark"}"#)
            .unwrap();
        assert!(store.save_custom_theme("../x", "{}").is_err());
        assert!(store.save_custom_theme("ok", "not json").is_err());
        assert_eq!(
            store.custom_themes(),
            vec![("My theme".to_string(), r#"{"base":"dark"}"#.to_string())]
        );
        store.delete_custom_theme("My theme").unwrap();
        assert!(store.custom_themes().is_empty());
    }
}
