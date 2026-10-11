//! The operating system's credential store, through the `keyring` crate: Windows Credential
//! Manager, macOS Keychain Services, the Secret Service on Linux and other Unix systems.
//!
//! Entries use the service name `logos` and the user name `<scope>/<NAME>`. The stores can't
//! list entries, so the names (never the values) are kept in `secrets-index.json` in the
//! configuration folder.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::{Scope, SecretStore, SecretValue, SecretsError, check_name, config_dir};

const SERVICE: &str = "logos";

pub struct KeyringStore {
    index_path: Option<PathBuf>,
}

impl KeyringStore {
    /// Opens the platform store, or fails if there is none (for example Linux without a
    /// Secret Service, as on a server or in a container).
    pub fn open() -> Result<KeyringStore, SecretsError> {
        if let Err(e) = keyring::Entry::store_status() {
            return Err(SecretsError::NoStore(e.to_string()));
        }
        Ok(KeyringStore {
            index_path: config_dir().map(|d| d.join("secrets-index.json")),
        })
    }

    fn entry(scope: &Scope, name: &str) -> Result<keyring::Entry, SecretsError> {
        keyring::Entry::new(SERVICE, &format!("{}/{name}", scope.key()))
            .map_err(|e| SecretsError::Store(e.to_string()))
    }

    fn read_index(&self) -> BTreeMap<String, BTreeSet<String>> {
        self.index_path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn update_index(&self, f: impl FnOnce(&mut BTreeMap<String, BTreeSet<String>>)) {
        let Some(path) = &self.index_path else { return };
        let mut index = self.read_index();
        f(&mut index);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(&index) {
            let _ = std::fs::write(path, text);
        }
    }
}

impl SecretStore for KeyringStore {
    fn describe(&self) -> String {
        if cfg!(windows) {
            "Windows Credential Manager".into()
        } else if cfg!(target_os = "macos") {
            "macOS Keychain".into()
        } else {
            "the Secret Service (desktop keyring)".into()
        }
    }

    fn get(&self, scope: &Scope, name: &str) -> Result<Option<SecretValue>, SecretsError> {
        match Self::entry(scope, name)?.get_password() {
            Ok(v) => Ok(Some(SecretValue::new(v))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(SecretsError::Store(e.to_string())),
        }
    }

    fn set(&mut self, scope: &Scope, name: &str, value: &SecretValue) -> Result<(), SecretsError> {
        check_name(name)?;
        Self::entry(scope, name)?
            .set_password(value.reveal())
            .map_err(|e| SecretsError::Store(e.to_string()))?;
        self.update_index(|i| {
            i.entry(scope.key()).or_default().insert(name.to_owned());
        });
        Ok(())
    }

    fn delete(&mut self, scope: &Scope, name: &str) -> Result<bool, SecretsError> {
        let had = match Self::entry(scope, name)?.delete_credential() {
            Ok(()) => true,
            Err(keyring::Error::NoEntry) => false,
            Err(e) => return Err(SecretsError::Store(e.to_string())),
        };
        self.update_index(|i| {
            if let Some(s) = i.get_mut(&scope.key()) {
                s.remove(name);
            }
        });
        Ok(had)
    }

    fn names(&self, scope: &Scope) -> Result<Vec<String>, SecretsError> {
        Ok(self
            .read_index()
            .remove(&scope.key())
            .map(|s| s.into_iter().collect())
            .unwrap_or_default())
    }
}
