//! Finding NetLogo and starting the run bridge (design 3.1, D-25).
//!
//! Logos looks for NetLogo in the order `repo-layout.md` gives ("Finding NetLogo"):
//! `LOGOS_NETLOGO_HOME`, the folder saved in the settings, then the default install folders.
//! It does not download or install NetLogo (D-31).

use std::path::{Path, PathBuf};

use logos_bridge_client::{BridgeConfig, NETLOGO_HOME_ENV};
use serde::Serialize;

/// The reference engine (D-05).
pub const NETLOGO_VERSION: &str = "7.0.4";

/// A NetLogo install found on this computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Install {
    pub path: PathBuf,
    /// From the name of `app/netlogo-<version>.jar`.
    pub version: String,
    /// Where Logos found it: `environment`, `settings`, `default` or `chosen`.
    pub source: String,
}

/// The version of the NetLogo install in `home`, from its jar's file name.
pub fn install_version(home: &Path) -> Option<String> {
    let entries = std::fs::read_dir(home.join("app")).ok()?;
    let mut versions: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let v = name.strip_prefix("netlogo-")?.strip_suffix(".jar")?;
            (v.chars().next()?.is_ascii_digit()).then(|| v.to_string())
        })
        .collect();
    versions.sort();
    versions.pop()
}

/// The folders that usually hold NetLogo installs on this platform.
fn default_parents() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"] {
            if let Some(base) = std::env::var_os(var) {
                out.push(PathBuf::from(base));
            }
        }
        out.push(PathBuf::from(r"C:\Program Files"));
    } else if cfg!(target_os = "macos") {
        out.push(PathBuf::from("/Applications"));
    } else {
        if let Some(home) = std::env::var_os("HOME") {
            out.push(PathBuf::from(home));
        }
        out.push(PathBuf::from("/opt"));
    }
    out
}

/// Every NetLogo install Logos can find, best first: the environment variable, the saved
/// folder, then `NetLogo <version>` folders in the default places (newest version first).
pub fn find_installs(saved: Option<&Path>) -> Vec<Install> {
    let mut out: Vec<Install> = Vec::new();
    let push = |path: PathBuf, source: &str, out: &mut Vec<Install>| {
        if out.iter().any(|i| i.path == path) {
            return;
        }
        if let Some(version) = install_version(&path) {
            out.push(Install {
                path,
                version,
                source: source.into(),
            });
        }
    };
    if let Some(home) = std::env::var_os(NETLOGO_HOME_ENV) {
        push(PathBuf::from(home), "environment", &mut out);
    }
    if let Some(saved) = saved {
        push(saved.to_path_buf(), "settings", &mut out);
    }
    let mut found = Vec::new();
    for parent in default_parents() {
        let Ok(entries) = std::fs::read_dir(&parent) else {
            continue;
        };
        for e in entries.filter_map(Result::ok) {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("NetLogo ") && e.path().is_dir() {
                found.push(e.path());
            }
        }
    }
    found.sort();
    found.reverse();
    for p in found {
        push(p, "default", &mut out);
    }
    out
}

/// The install to use: the first one with the reference version, or None.
pub fn choose(installs: &[Install]) -> Option<&Install> {
    installs.iter().find(|i| i.version == NETLOGO_VERSION)
}

/// The bridge jar: `LOGOS_BRIDGE_JAR`, then `bridge/build/libs` in the checkout the app was
/// built from, then next to the app's executable.
pub fn bridge_jar() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("LOGOS_BRIDGE_JAR") {
        return Some(PathBuf::from(p));
    }
    let name = "logos-bridge-0.0.0.jar";
    let checkout = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../bridge/build/libs")
        .join(name);
    if checkout.is_file() {
        return Some(checkout);
    }
    let beside = std::env::current_exe().ok()?.parent()?.join(name);
    beside.is_file().then_some(beside)
}

/// The configuration of the run bridge (D-25): NetLogo's own JVM options plus an explicit heap
/// limit, so that it and the check bridge fit together.
pub fn run_config(home: &Path, version: &str, jar: PathBuf, max_heap_mb: u32) -> BridgeConfig {
    let mut c = BridgeConfig::new(home.to_path_buf(), version, jar);
    if max_heap_mb > 0 {
        c.extra_jvm_args.push(format!("-Xmx{max_heap_mb}m"));
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_install(dir: &Path, version: &str) -> PathBuf {
        let home = dir.join(format!("NetLogo {version}"));
        std::fs::create_dir_all(home.join("app")).unwrap();
        std::fs::write(home.join("app").join(format!("netlogo-{version}.jar")), "").unwrap();
        std::fs::write(home.join("app").join("scala-library.jar"), "").unwrap();
        home
    }

    #[test]
    fn versions_and_choice() {
        let dir = tempfile::tempdir().unwrap();
        let a = fake_install(dir.path(), "7.0.4");
        let b = fake_install(dir.path(), "7.1.0");
        assert_eq!(install_version(&a).as_deref(), Some("7.0.4"));
        assert_eq!(install_version(dir.path()), None);
        let installs = find_installs(Some(&b));
        assert_eq!(installs[0].path, b);
        assert_eq!(installs[0].source, "settings");
        assert!(choose(&installs[..1]).is_none());
        let both = vec![
            installs[0].clone(),
            Install {
                path: a.clone(),
                version: "7.0.4".into(),
                source: "default".into(),
            },
        ];
        assert_eq!(choose(&both).unwrap().path, a);
    }
}
