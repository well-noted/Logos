//! A secrets file encrypted with a passphrase, for machines without a credential store (a
//! Linux server or container). Never a plain-text file (Part K, K4).
//!
//! Format, one item per line:
//!
//! ```text
//! logos-secrets 1
//! <salt, hex>          16 random bytes for Argon2id
//! <nonce, hex>         24 random bytes, new on every write
//! <ciphertext, hex>    XChaCha20-Poly1305 of JSON {scope: {name: value}}
//! ```
//!
//! The key is Argon2id (default parameters) of the passphrase and salt.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::aead::{Aead, KeyInit};

use crate::{Scope, SecretStore, SecretValue, SecretsError, check_name};

const HEADER: &str = "logos-secrets 1";

type Entries = BTreeMap<String, BTreeMap<String, SecretValue>>;

pub struct FileStore {
    path: PathBuf,
    salt: [u8; 16],
    key: [u8; 32],
    entries: Entries,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

fn derive(passphrase: &str, salt: &[u8]) -> Result<[u8; 32], SecretsError> {
    let mut key = [0u8; 32];
    argon2::Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| SecretsError::File(format!("key derivation: {e}")))?;
    Ok(key)
}

fn random<const N: usize>() -> Result<[u8; N], SecretsError> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|e| SecretsError::File(format!("random bytes: {e}")))?;
    Ok(b)
}

impl FileStore {
    /// The default location: `secrets.enc` in the configuration folder.
    pub fn default_path() -> Option<PathBuf> {
        crate::config_dir().map(|d| d.join("secrets.enc"))
    }

    pub fn exists(path: &Path) -> bool {
        path.is_file()
    }

    /// Opens the file with a passphrase, or starts an empty store if the file doesn't exist
    /// (nothing is written until the first change).
    pub fn open(path: impl Into<PathBuf>, passphrase: &str) -> Result<FileStore, SecretsError> {
        let path = path.into();
        if !path.exists() {
            let salt = random::<16>()?;
            return Ok(FileStore {
                key: derive(passphrase, &salt)?,
                path,
                salt,
                entries: Entries::new(),
            });
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| SecretsError::File(format!("{}: {e}", path.display())))?;
        let mut lines = text.lines();
        let bad = || SecretsError::File(format!("{} is not a Logos secrets file", path.display()));
        if lines.next() != Some(HEADER) {
            return Err(bad());
        }
        let salt: [u8; 16] = lines
            .next()
            .and_then(unhex)
            .and_then(|v| v.try_into().ok())
            .ok_or_else(bad)?;
        let nonce = lines
            .next()
            .and_then(unhex)
            .filter(|n| n.len() == 24)
            .ok_or_else(bad)?;
        let ciphertext = lines.next().and_then(unhex).ok_or_else(bad)?;
        let key = derive(passphrase, &salt)?;
        let cipher = XChaCha20Poly1305::new(&key.into());
        let nonce: [u8; 24] = nonce.try_into().unwrap();
        let plain = cipher
            .decrypt(&nonce.into(), ciphertext.as_ref())
            .map_err(|_| SecretsError::BadPassphrase)?;
        let raw: BTreeMap<String, BTreeMap<String, String>> =
            serde_json::from_slice(&plain).map_err(|_| SecretsError::BadPassphrase)?;
        let entries = raw
            .into_iter()
            .map(|(s, m)| {
                (
                    s,
                    m.into_iter()
                        .map(|(n, v)| (n, SecretValue::new(v)))
                        .collect(),
                )
            })
            .collect();
        Ok(FileStore {
            path,
            salt,
            key,
            entries,
        })
    }

    fn save(&self) -> Result<(), SecretsError> {
        let plain: BTreeMap<&String, BTreeMap<&String, &str>> = self
            .entries
            .iter()
            .map(|(s, m)| (s, m.iter().map(|(n, v)| (n, v.reveal())).collect()))
            .collect();
        let mut plain =
            serde_json::to_vec(&plain).map_err(|e| SecretsError::File(e.to_string()))?;
        let nonce = random::<24>()?;
        let cipher = XChaCha20Poly1305::new(&self.key.into());
        let ciphertext = cipher
            .encrypt(&nonce.into(), plain.as_ref())
            .map_err(|_| SecretsError::File("encryption failed".into()))?;
        plain.fill(0);
        let text = format!(
            "{HEADER}\n{}\n{}\n{}\n",
            hex(&self.salt),
            hex(&nonce),
            hex(&ciphertext)
        );
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| SecretsError::File(e.to_string()))?;
        }
        // Write a new file and rename it over the old one, so an interrupted write never
        // leaves a damaged store.
        let tmp = self.path.with_extension("enc.tmp");
        std::fs::write(&tmp, text).map_err(|e| SecretsError::File(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, &self.path).map_err(|e| SecretsError::File(e.to_string()))
    }
}

impl Drop for FileStore {
    fn drop(&mut self) {
        self.key.fill(0);
    }
}

impl SecretStore for FileStore {
    fn describe(&self) -> String {
        format!("encrypted file {}", self.path.display())
    }

    fn get(&self, scope: &Scope, name: &str) -> Result<Option<SecretValue>, SecretsError> {
        Ok(self
            .entries
            .get(&scope.key())
            .and_then(|m| m.get(name))
            .cloned())
    }

    fn set(&mut self, scope: &Scope, name: &str, value: &SecretValue) -> Result<(), SecretsError> {
        check_name(name)?;
        self.entries
            .entry(scope.key())
            .or_default()
            .insert(name.to_owned(), value.clone());
        self.save()
    }

    fn delete(&mut self, scope: &Scope, name: &str) -> Result<bool, SecretsError> {
        let had = self
            .entries
            .get_mut(&scope.key())
            .is_some_and(|m| m.remove(name).is_some());
        if had {
            self.save()?;
        }
        Ok(had)
    }

    fn names(&self, scope: &Scope) -> Result<Vec<String>, SecretsError> {
        Ok(self
            .entries
            .get(&scope.key())
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_wrong_passphrase() {
        let dir = std::env::temp_dir().join(format!("logos secrets file {}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("secrets.enc");
        let value = "sk-ant-api03-THE-SECRET-VALUE-1234";
        {
            let mut s = FileStore::open(&path, "correct horse").unwrap();
            assert!(!path.exists(), "nothing written before the first change");
            s.set(&Scope::User, "ANTHROPIC_API_KEY", &SecretValue::new(value))
                .unwrap();
            s.set(&Scope::Project("p1".into()), "X", &SecretValue::new("x"))
                .unwrap();
        }
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("logos-secrets 1\n"));
        assert!(
            !text.contains(value) && !text.contains("ANTHROPIC"),
            "encrypted, names too"
        );
        let first_nonce = text.lines().nth(2).unwrap().to_owned();

        let mut s = FileStore::open(&path, "correct horse").unwrap();
        assert_eq!(
            s.get(&Scope::User, "ANTHROPIC_API_KEY")
                .unwrap()
                .unwrap()
                .reveal(),
            value
        );
        assert_eq!(s.names(&Scope::Project("p1".into())).unwrap(), ["X"]);
        assert!(s.delete(&Scope::Project("p1".into()), "X").unwrap());
        assert!(!s.delete(&Scope::Project("p1".into()), "X").unwrap());
        let text = std::fs::read_to_string(&path).unwrap();
        assert_ne!(
            text.lines().nth(2).unwrap(),
            first_nonce,
            "new nonce per write"
        );

        assert!(matches!(
            FileStore::open(&path, "wrong"),
            Err(SecretsError::BadPassphrase)
        ));
        std::fs::write(&path, "not a secrets file").unwrap();
        assert!(matches!(
            FileStore::open(&path, "correct horse"),
            Err(SecretsError::File(_))
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
