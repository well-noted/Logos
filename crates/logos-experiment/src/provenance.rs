//! What a run depended on (Part E, E3): the engine, the operating system, and a SHA-256 hash
//! of every file the run read. Stored in `experiment.json` and in every run file, so a result
//! can be traced to the exact model and data that produced it, and a resumed experiment can
//! refuse to mix results from different inputs.

use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A file a run depended on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HashedFile {
    /// As listed in the project (relative to the project folder, `/` separators), or the
    /// absolute path of a file outside it.
    pub path: String,
    /// `model`, `project`, `experiments`, `dictionary`, or a project file role (`input`,
    /// `code`, `config`).
    pub role: String,
    /// Lowercase hex SHA-256 of the file's bytes; `None` if the file was missing.
    pub sha256: Option<String>,
}

impl HashedFile {
    /// Hashes the file at `full` and records it under `path`.
    pub fn new(path: &str, role: &str, full: &Path) -> HashedFile {
        HashedFile {
            path: path.to_owned(),
            role: role.to_owned(),
            sha256: hash_file(full).ok(),
        }
    }
}

/// The lowercase hex SHA-256 of a file.
pub fn hash_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    pub logos_version: String,
    pub netlogo_version: String,
    pub bridge_version: String,
    pub java_version: String,
    pub java_vendor: String,
    /// `windows`, `macos` or `linux`, as Rust names them.
    pub os: String,
    pub arch: String,
    pub files: Vec<HashedFile>,
    /// Names of the secrets passed to the engine. Never their values (Part K).
    pub secrets: Vec<String>,
}

impl Provenance {
    /// What differs between the provenance results were started with (`self`) and the
    /// current one, in ways that can change results: engine versions and file contents. The
    /// operating system, Java build and Logos version may differ; they are recorded per run.
    pub fn differences(&self, now: &Provenance) -> Vec<String> {
        let mut out = Vec::new();
        if self.netlogo_version != now.netlogo_version {
            out.push(format!(
                "NetLogo {} then, {} now",
                self.netlogo_version, now.netlogo_version
            ));
        }
        if self.bridge_version != now.bridge_version {
            out.push(format!(
                "bridge {} then, {} now",
                self.bridge_version, now.bridge_version
            ));
        }
        for f in &self.files {
            match now.files.iter().find(|g| g.path == f.path) {
                None => out.push(format!("{} is no longer part of the project", f.path)),
                Some(g) if g.sha256 != f.sha256 => out.push(match (&f.sha256, &g.sha256) {
                    (Some(_), None) => format!("{} is missing", f.path),
                    (None, Some(_)) => format!("{} was missing and now exists", f.path),
                    _ => format!("{} changed", f.path),
                }),
                Some(_) => {}
            }
        }
        for g in &now.files {
            if !self.files.iter().any(|f| f.path == g.path) {
                out.push(format!("{} was added to the project", g.path));
            }
        }
        out
    }
}

/// The per-run part of a run record; the run's settings, measurements, errors and step
/// count are in the bridge's result stored next to it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub run_number: u64,
    /// Local time with offset, RFC 3339.
    pub started: String,
    pub ended: String,
    /// The workspace (0-based) that ran it.
    pub worker: usize,
    /// The value of a `random-seed` setting, as BehaviorSpace writes it, when the experiment
    /// sets one. A seed the model sets in its own code is in the run's settings or not known.
    pub random_seed: Option<String>,
    pub provenance: Provenance,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provenance(files: Vec<HashedFile>) -> Provenance {
        Provenance {
            logos_version: "0.0.0".into(),
            netlogo_version: "7.0.4".into(),
            bridge_version: "0.0.0".into(),
            java_version: "17".into(),
            java_vendor: "x".into(),
            os: "windows".into(),
            arch: "x86_64".into(),
            files,
            secrets: vec!["OPENAI_API_KEY".into()],
        }
    }

    #[test]
    fn hashes_files_and_finds_changes() {
        let dir = std::env::temp_dir().join(format!("logos-provenance-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let data = dir.join("data.txt");
        std::fs::write(&data, "abc").unwrap();
        let before = HashedFile::new("data/data.txt", "input", &data);
        assert_eq!(
            before.sha256.as_deref(),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        std::fs::write(&data, "abd").unwrap();
        let after = HashedFile::new("data/data.txt", "input", &data);
        assert_ne!(before.sha256, after.sha256);
        let missing = HashedFile::new("data/gone.txt", "input", &dir.join("gone.txt"));
        assert_eq!(missing.sha256, None);

        let then = provenance(vec![before.clone(), missing.clone()]);
        assert!(then.differences(&then).is_empty());
        let mut now = provenance(vec![after, missing]);
        now.os = "linux".into();
        assert_eq!(then.differences(&now), vec!["data/data.txt changed"]);
        now.netlogo_version = "7.0.5".into();
        now.files.push(HashedFile {
            path: "x".into(),
            role: "code".into(),
            sha256: None,
        });
        assert_eq!(then.differences(&now).len(), 3);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
