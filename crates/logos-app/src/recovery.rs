//! Autosave and crash recovery (D-24, spec part I). Unsaved model text is copied to the app
//! data folder every 30 seconds and when the window loses focus; a clean save or closing
//! without saving removes the copy. A copy still there when the model is opened again means
//! the app stopped with unsaved edits, and the app offers to restore it with a diff against
//! the file on disk. Nothing is written into the project folder (D-19).

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub struct Autosaves {
    dir: PathBuf,
}

/// An autosaved copy of a model.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recovery {
    /// When the copy was written, RFC 3339 in local time.
    pub saved_at: String,
    /// The copy against the file on disk.
    pub diff: Vec<DiffLine>,
    /// Lines added and removed, over the whole file.
    pub added: usize,
    pub removed: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    path: PathBuf,
    saved_at: String,
}

impl Autosaves {
    pub fn new(dir: PathBuf) -> Autosaves {
        Autosaves { dir }
    }

    /// The file name for a model's copy: a hash of its path, so any path fits.
    fn key(path: &Path) -> String {
        let digest = Sha256::digest(path.to_string_lossy().as_bytes());
        digest[..12].iter().map(|b| format!("{b:02x}")).collect()
    }

    fn text_path(&self, path: &Path) -> PathBuf {
        self.dir.join(format!("{}.nlogox", Self::key(path)))
    }

    fn meta_path(&self, path: &Path) -> PathBuf {
        self.dir.join(format!("{}.json", Self::key(path)))
    }

    pub fn write(&self, path: &Path, text: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        // Skip the write when the copy already holds this text.
        if std::fs::read_to_string(self.text_path(path))
            .ok()
            .as_deref()
            == Some(text)
        {
            return Ok(());
        }
        crate::settings::write_atomically(&self.text_path(path), text.as_bytes())?;
        let meta = Meta {
            path: path.to_path_buf(),
            saved_at: chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        };
        let json = serde_json::to_string_pretty(&meta).map_err(std::io::Error::other)?;
        crate::settings::write_atomically(&self.meta_path(path), json.as_bytes())
    }

    pub fn remove(&self, path: &Path) {
        let _ = std::fs::remove_file(self.text_path(path));
        let _ = std::fs::remove_file(self.meta_path(path));
    }

    /// The autosaved text of a model and when it was written, if there is a copy.
    pub fn read(&self, path: &Path) -> Option<(String, String)> {
        let text = std::fs::read_to_string(self.text_path(path)).ok()?;
        let saved_at = std::fs::read_to_string(self.meta_path(path))
            .ok()
            .and_then(|j| serde_json::from_str::<serde_json::Value>(&j).ok())
            .and_then(|v| v.get("savedAt")?.as_str().map(str::to_string))
            .unwrap_or_default();
        Some((text, saved_at))
    }

    /// What restoring the copy would change, or None when there is no copy or it matches the
    /// file on disk (then the copy is removed).
    pub fn recovery(&self, path: &Path, disk: &str) -> Option<Recovery> {
        let (text, saved_at) = self.read(path)?;
        if text == disk {
            self.remove(path);
            return None;
        }
        let diff = line_diff(disk, &text, 3);
        let added = diff.iter().filter(|l| l.kind == DiffKind::Added).count();
        let removed = diff.iter().filter(|l| l.kind == DiffKind::Removed).count();
        Some(Recovery {
            saved_at,
            diff,
            added,
            removed,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiffKind {
    Same,
    Added,
    Removed,
    /// Unchanged lines left out between two changes.
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: DiffKind,
    pub text: String,
    /// 1-based line in the old text (same and removed lines) or the new text (added lines).
    pub line: usize,
}

/// Lines middle parts longer than this are compared as a whole block (removed, then added),
/// to keep the comparison fast.
const MAX_DP: usize = 3000;

/// A line diff from `old` to `new`, keeping `context` unchanged lines around each change.
pub fn line_diff(old: &str, new: &str, context: usize) -> Vec<DiffLine> {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (am, bm) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);

    // (kind, old index or new index)
    let mut ops: Vec<(DiffKind, usize)> = (0..prefix).map(|i| (DiffKind::Same, i)).collect();
    if am.len() <= MAX_DP && bm.len() <= MAX_DP {
        // Longest common subsequence over the middle part.
        let (n, m) = (am.len(), bm.len());
        let mut dp = vec![0u32; (n + 1) * (m + 1)];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                dp[i * (m + 1) + j] = if am[i] == bm[j] {
                    dp[(i + 1) * (m + 1) + j + 1] + 1
                } else {
                    dp[(i + 1) * (m + 1) + j].max(dp[i * (m + 1) + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n || j < m {
            if i < n && j < m && am[i] == bm[j] {
                ops.push((DiffKind::Same, prefix + i));
                i += 1;
                j += 1;
            } else if i < n && (j == m || dp[(i + 1) * (m + 1) + j] >= dp[i * (m + 1) + j + 1]) {
                // Removed lines come before added ones at the same place.
                ops.push((DiffKind::Removed, prefix + i));
                i += 1;
            } else {
                ops.push((DiffKind::Added, prefix + j));
                j += 1;
            }
        }
    } else {
        ops.extend((0..am.len()).map(|i| (DiffKind::Removed, prefix + i)));
        ops.extend((0..bm.len()).map(|j| (DiffKind::Added, prefix + j)));
    }
    let tail = a.len() - suffix;
    ops.extend((0..suffix).map(|k| (DiffKind::Same, tail + k)));

    // Keep changes and their context; collapse the rest.
    let changed: Vec<bool> = ops.iter().map(|(k, _)| *k != DiffKind::Same).collect();
    let near = |idx: usize| {
        let lo = idx.saturating_sub(context);
        let hi = (idx + context + 1).min(ops.len());
        changed[lo..hi].iter().any(|&c| c)
    };
    let mut out = Vec::new();
    let mut skipped = 0;
    for (idx, (kind, i)) in ops.iter().enumerate() {
        if *kind == DiffKind::Same && !near(idx) {
            skipped += 1;
            continue;
        }
        if skipped > 0 {
            out.push(DiffLine {
                kind: DiffKind::Skipped,
                text: format!("{skipped} unchanged lines"),
                line: 0,
            });
            skipped = 0;
        }
        let text = match kind {
            DiffKind::Added => b[*i],
            _ => a[*i],
        };
        out.push(DiffLine {
            kind: *kind,
            text: text.to_string(),
            line: i + 1,
        });
    }
    if skipped > 0 {
        out.push(DiffLine {
            kind: DiffKind::Skipped,
            text: format!("{skipped} unchanged lines"),
            line: 0,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(d: &[DiffLine]) -> String {
        d.iter()
            .map(|l| match l.kind {
                DiffKind::Same => '=',
                DiffKind::Added => '+',
                DiffKind::Removed => '-',
                DiffKind::Skipped => '~',
            })
            .collect()
    }

    #[test]
    fn diff_keeps_context_and_collapses_the_rest() {
        let old: String = (1..=20).map(|i| format!("line {i}\n")).collect();
        let new = old.replace("line 10\n", "line ten\n");
        let d = line_diff(&old, &new, 2);
        assert_eq!(kinds(&d), "~==-+==~");
        assert_eq!(d[3].text, "line 10");
        assert_eq!(d[4].text, "line ten");
        assert_eq!(d[0].text, "7 unchanged lines");
    }

    #[test]
    fn diff_of_insertions_and_deletions() {
        let d = line_diff("a\nb\nc\n", "a\nx\nc\nd\n", 5);
        assert_eq!(kinds(&d), "=-+=+");
        assert!(
            line_diff("same\n", "same\n", 3)
                .iter()
                .all(|l| l.kind != DiffKind::Added)
        );
    }

    #[test]
    fn autosave_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let saves = Autosaves::new(dir.path().join("autosave"));
        let model = Path::new("C:/Users/someone/My Models/a model.nlogox");
        assert!(saves.read(model).is_none());
        saves.write(model, "<model>new</model>").unwrap();
        let r = saves.recovery(model, "<model>old</model>").unwrap();
        assert_eq!((r.added, r.removed), (1, 1));
        assert!(!r.saved_at.is_empty());
        // A copy that matches the disk is not offered, and is removed.
        assert!(saves.recovery(model, "<model>new</model>").is_none());
        assert!(saves.read(model).is_none());
    }
}
