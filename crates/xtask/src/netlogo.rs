//! Locating a NetLogo installation.
//!
//! Search order (docs/repo-layout.md, "Finding NetLogo"):
//! 1. `LOGOS_NETLOGO_HOME`, if it holds the requested version.
//! 2. The default install folder for the platform.
//!
//! A folder counts as NetLogo `<version>` when it contains `app/netlogo-<version>.jar`.

use std::path::{Path, PathBuf};

pub const HOME_ENV: &str = "LOGOS_NETLOGO_HOME";

/// The jar that identifies an install of `version`, relative to the install folder.
pub fn jar_path(home: &Path, version: &str) -> PathBuf {
    home.join("app").join(format!("netlogo-{version}.jar"))
}

pub fn is_install_of(home: &Path, version: &str) -> bool {
    jar_path(home, version).is_file()
}

/// Default install folders to try for `version` on this platform.
pub fn default_locations(version: &str) -> Vec<PathBuf> {
    let name = format!("NetLogo {version}");
    let mut out = Vec::new();
    if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"] {
            if let Some(base) = std::env::var_os(var) {
                out.push(PathBuf::from(base).join(&name));
            }
        }
        out.push(PathBuf::from(r"C:\Program Files").join(&name));
    } else if cfg!(target_os = "macos") {
        out.push(PathBuf::from("/Applications").join(&name));
    } else {
        if let Some(home) = std::env::var_os("HOME") {
            out.push(PathBuf::from(home).join(&name));
        }
        out.push(PathBuf::from("/opt").join(&name));
    }
    out.dedup();
    out
}

/// Finds an install of `version`, or None.
pub fn find_home(version: &str) -> Option<PathBuf> {
    if let Some(home) = std::env::var_os(HOME_ENV).map(PathBuf::from)
        && is_install_of(&home, version)
    {
        return Some(home);
    }
    default_locations(version)
        .into_iter()
        .find(|p| is_install_of(p, version))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_install_by_jar() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_install_of(dir.path(), "7.0.4"));
        std::fs::create_dir_all(dir.path().join("app")).unwrap();
        std::fs::write(dir.path().join("app/netlogo-7.0.4.jar"), b"").unwrap();
        assert!(is_install_of(dir.path(), "7.0.4"));
        assert!(!is_install_of(dir.path(), "7.1.0"));
    }

    #[test]
    fn default_locations_name_the_version() {
        for p in default_locations("7.0.4") {
            assert!(p.ends_with("NetLogo 7.0.4"), "{}", p.display());
        }
    }
}
