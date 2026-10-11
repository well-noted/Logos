//! Secrets for Logos projects (Phase 1, Part K): API keys and other environment variables a
//! model needs.
//!
//! Rules (docs/phase-1-spec.md, Part K):
//!
//! - Values live in the operating system's credential store ([`KeyringStore`]), or, where
//!   there is none, in a file encrypted with a passphrase ([`FileStore`]). Never in project
//!   files, run records or logs.
//! - A secret belongs to one project or to the user. A run looks in the project, then the
//!   user, then the environment Logos was started with ([`resolve`]).
//! - Values are shown masked ([`mask`]). Unmasking is logged by name and time ([`log_access`]).
//! - [`SecretValue`] never prints its value through `Debug` or `Display`.

pub mod dotenv;
mod file;
mod keyring_store;

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

pub use file::FileStore;
pub use keyring_store::KeyringStore;

/// Where a secret belongs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    /// Shared by all of the user's projects (most API keys).
    User,
    /// One project, by the id in its `logos.toml`.
    Project(String),
}

impl Scope {
    /// The scope as stored: `user` or `project:<id>`.
    pub fn key(&self) -> String {
        match self {
            Scope::User => "user".into(),
            Scope::Project(id) => format!("project:{id}"),
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Scope::User => f.write_str("user"),
            Scope::Project(_) => f.write_str("project"),
        }
    }
}

/// A secret's value. `Debug` and `Display` print `[hidden]`; use [`SecretValue::reveal`] to
/// get the text, and only where it is needed (starting a process, an explicit unmask).
#[derive(Clone, PartialEq, Eq)]
pub struct SecretValue(String);

impl SecretValue {
    pub fn new(value: impl Into<String>) -> SecretValue {
        SecretValue(value.into())
    }

    pub fn reveal(&self) -> &str {
        &self.0
    }

    pub fn masked(&self) -> String {
        mask(&self.0)
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[hidden]")
    }
}

impl fmt::Display for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[hidden]")
    }
}

impl Drop for SecretValue {
    fn drop(&mut self) {
        // Overwrite the text before the memory is freed.
        // SAFETY: writing zero bytes keeps the string valid UTF-8.
        unsafe { self.0.as_bytes_mut().fill(0) };
    }
}

/// A value shown masked: the first three and last four characters of a long value
/// (`sk-…3f2a`), dots for a short one.
pub fn mask(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() >= 16 {
        let head: String = chars[..3].iter().collect();
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("{head}…{tail}")
    } else {
        "••••".into()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SecretsError {
    #[error("no credential store is available: {0}")]
    NoStore(String),
    #[error("credential store: {0}")]
    Store(String),
    #[error("wrong passphrase, or the secrets file is damaged")]
    BadPassphrase,
    #[error("{0}")]
    File(String),
    #[error("`{0}` is not an environment variable name")]
    BadName(String),
}

/// Somewhere secrets are kept.
pub trait SecretStore {
    /// A short description for messages: `Windows Credential Manager`, `encrypted file ...`.
    fn describe(&self) -> String;
    fn get(&self, scope: &Scope, name: &str) -> Result<Option<SecretValue>, SecretsError>;
    fn set(&mut self, scope: &Scope, name: &str, value: &SecretValue) -> Result<(), SecretsError>;
    /// Returns whether there was a value.
    fn delete(&mut self, scope: &Scope, name: &str) -> Result<bool, SecretsError>;
    /// The names stored in a scope. Names are not secret.
    fn names(&self, scope: &Scope) -> Result<Vec<String>, SecretsError>;
}

/// Secret names are environment variable names: letters, digits and `_`, not starting with a
/// digit.
pub fn check_name(name: &str) -> Result<(), SecretsError> {
    let ok = !name.is_empty()
        && !name.as_bytes()[0].is_ascii_digit()
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(SecretsError::BadName(name.to_owned()))
    }
}

/// Chooses the store: `LOGOS_SECRETS_STORE=os` or `file` forces one; otherwise the
/// operating system's credential store when there is one, and the encrypted file when there
/// isn't. `passphrase` is asked for the file's passphrase (its argument says whether the file
/// is new); `LOGOS_SECRETS_PASSPHRASE`, for servers where no one can type it, is used first if
/// set. `LOGOS_SECRETS_FILE` overrides the file's location.
pub fn open_store(
    passphrase: &dyn Fn(bool) -> Result<String, SecretsError>,
) -> Result<Box<dyn SecretStore>, SecretsError> {
    let forced = std::env::var("LOGOS_SECRETS_STORE").ok();
    if forced.as_deref() != Some("file") {
        match KeyringStore::open() {
            Ok(s) => return Ok(Box::new(s)),
            Err(e) if forced.as_deref() == Some("os") => return Err(e),
            Err(_) => {}
        }
    }
    let path = std::env::var_os("LOGOS_SECRETS_FILE")
        .map(PathBuf::from)
        .or_else(FileStore::default_path)
        .ok_or_else(|| SecretsError::File("no configuration folder for the secrets file".into()))?;
    let pass = match std::env::var("LOGOS_SECRETS_PASSPHRASE") {
        Ok(p) if !p.is_empty() => p,
        _ => passphrase(!FileStore::exists(&path))?,
    };
    Ok(Box::new(FileStore::open(path, &pass)?))
}

/// Where a resolved secret came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Project,
    User,
    /// The environment Logos was started with.
    Environment,
}

/// The value a run gets for each name: from the project, else the user, else the environment.
/// Names with no value anywhere are left out.
pub fn resolve(
    store: &dyn SecretStore,
    project_id: Option<&str>,
    names: &[String],
) -> Result<BTreeMap<String, (SecretValue, Source)>, SecretsError> {
    let mut out = BTreeMap::new();
    for name in names {
        if let Some(id) = project_id
            && let Some(v) = store.get(&Scope::Project(id.to_owned()), name)?
        {
            out.insert(name.clone(), (v, Source::Project));
            continue;
        }
        if let Some(v) = store.get(&Scope::User, name)? {
            out.insert(name.clone(), (v, Source::User));
            continue;
        }
        if let Some(v) = std::env::var_os(name).filter(|v| !v.is_empty()) {
            out.insert(
                name.clone(),
                (
                    SecretValue::new(v.to_string_lossy().into_owned()),
                    Source::Environment,
                ),
            );
        }
    }
    Ok(out)
}

/// Logos's per-user configuration folder: `LOGOS_CONFIG_DIR` if set, otherwise
/// `%APPDATA%\Logos` on Windows, `~/Library/Application Support/Logos` on macOS, and
/// `$XDG_CONFIG_HOME/logos` or `~/.config/logos` elsewhere.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("LOGOS_CONFIG_DIR") {
        return Some(PathBuf::from(d));
    }
    if cfg!(windows) {
        return std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Logos"));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        return home.map(|h| h.join("Library/Application Support/Logos"));
    }
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".config")))
        .map(|c| c.join("logos"))
}

/// The current time as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Records that a secret was unmasked or changed: time, action, scope and name, never the
/// value. Appends to `secrets-access.log` in the configuration folder.
pub fn log_access(action: &str, scope: &Scope, name: &str) -> Result<(), SecretsError> {
    use std::io::Write;
    let dir = config_dir().ok_or_else(|| SecretsError::File("no configuration folder".into()))?;
    std::fs::create_dir_all(&dir).map_err(|e| SecretsError::File(e.to_string()))?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("secrets-access.log"))
        .map_err(|e| SecretsError::File(e.to_string()))?;
    writeln!(f, "{} {action} {} {name}", utc_now(), scope.key())
        .map_err(|e| SecretsError::File(e.to_string()))
}

/// A store kept in memory, for tests.
#[derive(Debug, Default)]
pub struct MemoryStore(pub BTreeMap<(String, String), SecretValue>);

impl SecretStore for MemoryStore {
    fn describe(&self) -> String {
        "memory".into()
    }
    fn get(&self, scope: &Scope, name: &str) -> Result<Option<SecretValue>, SecretsError> {
        Ok(self.0.get(&(scope.key(), name.to_owned())).cloned())
    }
    fn set(&mut self, scope: &Scope, name: &str, value: &SecretValue) -> Result<(), SecretsError> {
        check_name(name)?;
        self.0.insert((scope.key(), name.to_owned()), value.clone());
        Ok(())
    }
    fn delete(&mut self, scope: &Scope, name: &str) -> Result<bool, SecretsError> {
        Ok(self.0.remove(&(scope.key(), name.to_owned())).is_some())
    }
    fn names(&self, scope: &Scope) -> Result<Vec<String>, SecretsError> {
        let k = scope.key();
        Ok(self
            .0
            .keys()
            .filter(|(s, _)| *s == k)
            .map(|(_, n)| n.clone())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_never_print() {
        let v = SecretValue::new("sk-ant-api03-abcdefghijklmnop3f2a");
        assert_eq!(format!("{v:?} {v}"), "[hidden] [hidden]");
        assert_eq!(v.masked(), "sk-…3f2a");
        assert_eq!(mask("short"), "••••");
        assert_eq!(v.reveal(), "sk-ant-api03-abcdefghijklmnop3f2a");
    }

    #[test]
    fn names_are_environment_variable_names() {
        assert!(check_name("ANTHROPIC_API_KEY").is_ok());
        assert!(check_name("_X1").is_ok());
        for bad in ["", "1X", "MY-KEY", "A B", "É"] {
            assert!(check_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn resolution_order() {
        let mut s = MemoryStore::default();
        let p = Scope::Project("abc".into());
        s.set(&p, "A", &SecretValue::new("project-a")).unwrap();
        s.set(&Scope::User, "A", &SecretValue::new("user-a"))
            .unwrap();
        s.set(&Scope::User, "B", &SecretValue::new("user-b"))
            .unwrap();
        let names: Vec<String> = ["A", "B", "LOGOS_TEST_UNSET_NAME", "PATH"]
            .map(String::from)
            .to_vec();
        let r = resolve(&s, Some("abc"), &names).unwrap();
        assert_eq!(r["A"].0.reveal(), "project-a");
        assert_eq!(r["A"].1, Source::Project);
        assert_eq!(r["B"].1, Source::User);
        assert_eq!(r["PATH"].1, Source::Environment);
        assert!(!r.contains_key("LOGOS_TEST_UNSET_NAME"));
        let r = resolve(&s, None, &names).unwrap();
        assert_eq!(r["A"].0.reveal(), "user-a");
    }

    #[test]
    fn time_format() {
        let t = utc_now();
        assert_eq!(t.len(), 20);
        assert!(t.starts_with("20") && t.ends_with('Z'));
    }
}
