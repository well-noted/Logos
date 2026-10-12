//! The git branch and the number of changed files for the status bar (spec part R). The Git
//! panel itself is Phase 3 (D-28). Runs the `git` program on the PATH; without it, or outside
//! a repository, the status bar shows nothing.

use serde::Serialize;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    /// The branch, or a short commit name when HEAD is detached.
    pub branch: String,
    /// Files with changes, staged or not, and untracked files.
    pub changed: usize,
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir).args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console window flashing up every time the status refreshes.
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The status of the repository containing `dir`, if it is in one.
pub fn status(dir: &Path) -> Option<GitStatus> {
    // symbolic-ref also works on a branch with no commits yet; it fails when HEAD is detached.
    let branch = match git(dir, &["symbolic-ref", "--short", "-q", "HEAD"]) {
        Some(b) => b.trim().to_string(),
        None => git(dir, &["rev-parse", "--short", "HEAD"])?
            .trim()
            .to_string(),
    };
    let porcelain = git(dir, &["status", "--porcelain=v1", "-z"])?;
    Some(GitStatus {
        branch,
        changed: count_changed(&porcelain),
    })
}

/// Counts entries in `git status --porcelain=v1 -z` output. A rename or copy entry is
/// followed by its original path as a separate field, which isn't a second file.
fn count_changed(porcelain: &str) -> usize {
    let mut n = 0;
    let mut fields = porcelain.split('\0').filter(|f| !f.is_empty());
    while let Some(f) = fields.next() {
        n += 1;
        let xy = f.get(..2).unwrap_or("");
        if xy.contains('R') || xy.contains('C') {
            fields.next();
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_entries_and_skips_rename_sources() {
        assert_eq!(count_changed(""), 0);
        assert_eq!(count_changed(" M a.txt\0?? b.txt\0"), 2);
        assert_eq!(count_changed("R  new.txt\0old.txt\0 M c.txt\0"), 2);
    }

    #[test]
    fn a_new_repository() {
        let dir = std::env::temp_dir().join(format!("logos git test {}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        if git(&dir, &["init", "-q", "-b", "main"]).is_none() {
            return; // no git on this machine
        }
        std::fs::write(dir.join("model.nlogox"), "x").unwrap();
        let s = status(&dir).unwrap();
        assert_eq!(s.changed, 1);
        assert_eq!(s.branch, "main");
        std::fs::remove_dir_all(&dir).ok();
    }
}
