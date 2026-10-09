//! The conformance corpus manifest (`corpus/manifest.toml`).
//!
//! The manifest lists every model used by the round-trip and conformance tests.
//! Models are not all stored in the repository: some live in the user's NetLogo
//! install (the Models Library) and some are downloaded. The manifest records
//! where each one comes from and its SHA-256 hash, so a test run can prove it is
//! using exactly the files the reference results were recorded from.
//!
//! The format is documented in `corpus/README.md`.

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The only manifest format version this code understands.
pub const MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    #[serde(default, rename = "model")]
    pub models: Vec<ModelEntry>,
    #[serde(default, rename = "collection")]
    pub collections: Vec<Collection>,
}

/// Where a corpus file comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// Committed to this repository, under `corpus/`.
    Repo,
    /// Part of a NetLogo installation (for example the Models Library).
    NetlogoInstall,
    /// Downloaded from `url` into `corpus/.cache/`.
    Url,
}

/// One model file with a known hash.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ModelEntry {
    pub id: String,
    /// Corpus set from the Phase 0 spec, for example "C3".
    pub set: String,
    pub source: Source,
    /// Relative path. For `repo`, relative to `corpus/`. For `netlogo-install`,
    /// relative to the NetLogo install folder. For `url`, the file name inside
    /// `corpus/.cache/<id>/`.
    pub path: String,
    pub url: Option<String>,
    pub sha256: String,
    /// SPDX identifier where one exists, otherwise a short description.
    pub license: String,
    /// NetLogo version the file belongs to. Required for `netlogo-install`.
    pub netlogo: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub notes: Option<String>,
}

/// A group of files selected by folder and extension, without per-file hashes.
/// Used for the whole Models Library of a pinned NetLogo version.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Collection {
    pub id: String,
    pub set: String,
    pub source: Source,
    /// Folder relative to the NetLogo install.
    pub root: String,
    /// File extensions to include, without the dot, for example ["nlogox"].
    pub extensions: Vec<String>,
    pub netlogo: String,
    pub license: String,
    pub notes: Option<String>,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest> {
        let manifest: Manifest = toml::from_str(text).context("corpus manifest is not valid")?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn load(path: &Path) -> Result<Manifest> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        Manifest::parse(&text).with_context(|| format!("in {}", path.display()))
    }

    /// Checks the rules that serde can't express.
    pub fn validate(&self) -> Result<()> {
        if self.version != MANIFEST_VERSION {
            bail!(
                "manifest version is {}, this code supports {}",
                self.version,
                MANIFEST_VERSION
            );
        }
        let mut ids = HashSet::new();
        for m in &self.models {
            check_id(&m.id)?;
            if !ids.insert(m.id.as_str()) {
                bail!("duplicate id `{}`", m.id);
            }
            check_relative_path(&m.path).with_context(|| format!("model `{}`", m.id))?;
            check_sha256(&m.sha256).with_context(|| format!("model `{}`", m.id))?;
            match m.source {
                Source::Url if m.url.is_none() => {
                    bail!("model `{}` has source = \"url\" but no url", m.id)
                }
                Source::Repo | Source::NetlogoInstall if m.url.is_some() => {
                    bail!("model `{}` has a url but its source is not \"url\"", m.id)
                }
                _ => {}
            }
            if m.source == Source::NetlogoInstall && m.netlogo.is_none() {
                bail!(
                    "model `{}` comes from a NetLogo install and must name its NetLogo version",
                    m.id
                );
            }
            if m.source == Source::Url && m.path.contains(['/', '\\']) {
                bail!(
                    "model `{}`: for url sources, path is a plain file name",
                    m.id
                );
            }
        }
        for c in &self.collections {
            check_id(&c.id)?;
            if !ids.insert(c.id.as_str()) {
                bail!("duplicate id `{}`", c.id);
            }
            if c.source != Source::NetlogoInstall {
                bail!(
                    "collection `{}`: only source = \"netlogo-install\" is supported",
                    c.id
                );
            }
            check_relative_path(&c.root).with_context(|| format!("collection `{}`", c.id))?;
            if c.extensions.is_empty() {
                bail!("collection `{}` lists no extensions", c.id);
            }
        }
        Ok(())
    }
}

fn check_id(id: &str) -> Result<()> {
    let ok = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.');
    if !ok {
        bail!("id `{id}` may only contain lowercase letters, digits, `-` and `.`");
    }
    Ok(())
}

fn check_sha256(hash: &str) -> Result<()> {
    let ok = hash.len() == 64
        && hash
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c));
    if !ok {
        bail!("sha256 must be 64 lowercase hex characters, got `{hash}`");
    }
    Ok(())
}

/// Paths in the manifest use `/` and must stay inside their base folder.
fn check_relative_path(path: &str) -> Result<()> {
    if path.is_empty() {
        bail!("path is empty");
    }
    if path.contains('\\') {
        bail!("path `{path}` must use `/` as the separator");
    }
    let p = Path::new(path);
    for component in p.components() {
        match component {
            Component::Normal(_) => {}
            _ => bail!("path `{path}` must be relative and must not contain `..`"),
        }
    }
    Ok(())
}

/// Finds a NetLogo install folder for a version, or None if not installed.
pub type NetlogoLocator = Box<dyn Fn(&str) -> Option<PathBuf>>;

/// Folders the manifest's relative paths are resolved against.
pub struct Roots {
    /// The repository's `corpus/` folder.
    pub corpus_dir: PathBuf,
    pub netlogo_home: NetlogoLocator,
}

impl Roots {
    pub fn cache_dir(&self) -> PathBuf {
        self.corpus_dir.join(".cache")
    }

    /// Absolute location of a model file, or the reason it can't be located.
    pub fn locate(&self, m: &ModelEntry) -> std::result::Result<PathBuf, String> {
        match m.source {
            Source::Repo => Ok(join_manifest_path(&self.corpus_dir, &m.path)),
            Source::Url => Ok(self.cache_dir().join(&m.id).join(&m.path)),
            Source::NetlogoInstall => {
                let version = m.netlogo.as_deref().unwrap_or_default();
                match (self.netlogo_home)(version) {
                    Some(home) => Ok(join_manifest_path(&home, &m.path)),
                    None => Err(format!("NetLogo {version} is not installed")),
                }
            }
        }
    }

    /// All files in a collection, sorted, or the reason they can't be listed.
    pub fn collection_files(&self, c: &Collection) -> std::result::Result<Vec<PathBuf>, String> {
        let home = (self.netlogo_home)(&c.netlogo)
            .ok_or_else(|| format!("NetLogo {} is not installed", c.netlogo))?;
        let root = join_manifest_path(&home, &c.root);
        if !root.is_dir() {
            return Err(format!("{} is not a folder", root.display()));
        }
        let mut files: Vec<PathBuf> = walkdir::WalkDir::new(&root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| c.extensions.iter().any(|want| want == x))
            })
            .map(|e| e.into_path())
            .collect();
        files.sort();
        Ok(files)
    }
}

fn join_manifest_path(base: &Path, relative: &str) -> PathBuf {
    let mut out = base.to_path_buf();
    for part in relative.split('/') {
        out.push(part);
    }
    out
}

/// Result of checking one model file against its recorded hash.
#[derive(Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    Missing,
    HashMismatch {
        actual: String,
    },
    /// The file's base location isn't available on this machine (for example
    /// NetLogo isn't installed). Not a failure unless the caller requires it.
    Unavailable(String),
}

pub fn check_model(roots: &Roots, m: &ModelEntry) -> Result<Status> {
    let path = match roots.locate(m) {
        Ok(p) => p,
        Err(reason) => return Ok(Status::Unavailable(reason)),
    };
    if !path.is_file() {
        return Ok(Status::Missing);
    }
    let actual = sha256_file(&path)?;
    if actual == m.sha256 {
        Ok(Status::Ok)
    } else {
        Ok(Status::HashMismatch { actual })
    }
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    sha256_reader(file).with_context(|| format!("cannot read {}", path.display()))
}

pub fn sha256_reader(mut reader: impl Read) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
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

/// Downloads a `url` model into the cache if it is missing or has the wrong hash.
/// The file is written to a temporary name and renamed only after its hash
/// matches, so a failed download never leaves a bad file in place.
pub fn fetch_model(roots: &Roots, m: &ModelEntry) -> Result<Status> {
    if m.source != Source::Url {
        return check_model(roots, m);
    }
    if check_model(roots, m)? == Status::Ok {
        return Ok(Status::Ok);
    }
    let url = m.url.as_deref().expect("validated: url sources have a url");
    let dest = roots.locate(m).expect("url sources always resolve");
    let dir = dest.parent().expect("cache path has a parent");
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let partial = dest.with_extension("partial");

    let response = ureq::get(url)
        .call()
        .with_context(|| format!("download of {url} failed"))?;
    let mut body = response.into_body().into_reader();
    let mut out =
        File::create(&partial).with_context(|| format!("cannot create {}", partial.display()))?;
    io::copy(&mut body, &mut out).with_context(|| format!("download of {url} failed"))?;
    drop(out);

    let actual = sha256_file(&partial)?;
    if actual != m.sha256 {
        let _ = std::fs::remove_file(&partial);
        return Ok(Status::HashMismatch { actual });
    }
    std::fs::rename(&partial, &dest)
        .with_context(|| format!("cannot move download to {}", dest.display()))?;
    Ok(Status::Ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH_A: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    fn manifest(body: &str) -> Result<Manifest> {
        Manifest::parse(&format!("version = 1\n{body}"))
    }

    fn roots_with(corpus: &Path, netlogo: Option<PathBuf>) -> Roots {
        Roots {
            corpus_dir: corpus.to_path_buf(),
            netlogo_home: Box::new(move |_| netlogo.clone()),
        }
    }

    #[test]
    fn parses_all_three_sources() {
        let m = manifest(&format!(
            r#"
[[model]]
id = "a"
set = "C3"
source = "repo"
path = "own/a/A.nlogox"
sha256 = "{HASH_A}"
license = "MIT"

[[model]]
id = "b"
set = "C1"
source = "netlogo-install"
path = "models/B.nlogox"
netlogo = "7.0.4"
sha256 = "{HASH_A}"
license = "CC-BY-NC-SA-3.0"
tags = ["gis"]

[[model]]
id = "c"
set = "C4"
source = "url"
url = "https://example.org/c.nlogox"
path = "c.nlogox"
sha256 = "{HASH_A}"
license = "GPL-3.0-only"

[[collection]]
id = "lib"
set = "C1"
source = "netlogo-install"
root = "models"
extensions = ["nlogox"]
netlogo = "7.0.4"
license = "various"
"#
        ))
        .unwrap();
        assert_eq!(m.models.len(), 3);
        assert_eq!(m.models[1].source, Source::NetlogoInstall);
        assert_eq!(m.models[1].tags, vec!["gis"]);
        assert_eq!(m.collections.len(), 1);
    }

    #[test]
    fn rejects_wrong_version() {
        assert!(Manifest::parse("version = 2").is_err());
    }

    #[test]
    fn rejects_unknown_fields() {
        let err = manifest(&format!(
            "[[model]]\nid = \"a\"\nset = \"C3\"\nsource = \"repo\"\npath = \"a\"\nsha256 = \"{HASH_A}\"\nlicense = \"MIT\"\ncolour = \"red\"\n"
        ));
        assert!(err.is_err());
    }

    #[test]
    fn rejects_duplicate_ids() {
        let entry = format!(
            "[[model]]\nid = \"a\"\nset = \"C3\"\nsource = \"repo\"\npath = \"a\"\nsha256 = \"{HASH_A}\"\nlicense = \"MIT\"\n"
        );
        let err = manifest(&format!("{entry}{entry}")).unwrap_err();
        assert!(format!("{err:#}").contains("duplicate id"));
    }

    #[test]
    fn rejects_url_source_without_url() {
        let err = manifest(&format!(
            "[[model]]\nid = \"a\"\nset = \"C4\"\nsource = \"url\"\npath = \"a\"\nsha256 = \"{HASH_A}\"\nlicense = \"MIT\"\n"
        ))
        .unwrap_err();
        assert!(format!("{err:#}").contains("no url"));
    }

    #[test]
    fn rejects_install_source_without_version() {
        let err = manifest(&format!(
            "[[model]]\nid = \"a\"\nset = \"C1\"\nsource = \"netlogo-install\"\npath = \"a\"\nsha256 = \"{HASH_A}\"\nlicense = \"MIT\"\n"
        ))
        .unwrap_err();
        assert!(format!("{err:#}").contains("NetLogo version"));
    }

    #[test]
    fn rejects_bad_hash_and_paths() {
        for (path, hash) in [
            ("a", "ABC"),
            ("../escape", HASH_A),
            ("/abs", HASH_A),
            ("back\\slash", HASH_A),
        ] {
            let text = format!(
                "[[model]]\nid = \"a\"\nset = \"C3\"\nsource = \"repo\"\npath = \"{}\"\nsha256 = \"{hash}\"\nlicense = \"MIT\"\n",
                path.replace('\\', "\\\\")
            );
            assert!(
                manifest(&text).is_err(),
                "should reject path={path} hash={hash}"
            );
        }
    }

    #[test]
    fn checks_hashes_of_repo_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("own")).unwrap();
        std::fs::write(dir.path().join("own/m.nlogox"), b"hello").unwrap();
        let good = sha256_reader(&b"hello"[..]).unwrap();
        let roots = roots_with(dir.path(), None);

        let m = manifest(&format!(
            "[[model]]\nid = \"m\"\nset = \"C3\"\nsource = \"repo\"\npath = \"own/m.nlogox\"\nsha256 = \"{good}\"\nlicense = \"MIT\"\n\n[[model]]\nid = \"bad\"\nset = \"C3\"\nsource = \"repo\"\npath = \"own/m.nlogox\"\nsha256 = \"{HASH_A}\"\nlicense = \"MIT\"\n\n[[model]]\nid = \"gone\"\nset = \"C3\"\nsource = \"repo\"\npath = \"own/none.nlogox\"\nsha256 = \"{HASH_A}\"\nlicense = \"MIT\"\n"
        ))
        .unwrap();
        assert_eq!(check_model(&roots, &m.models[0]).unwrap(), Status::Ok);
        assert_eq!(
            check_model(&roots, &m.models[1]).unwrap(),
            Status::HashMismatch { actual: good }
        );
        assert_eq!(check_model(&roots, &m.models[2]).unwrap(), Status::Missing);
    }

    #[test]
    fn install_files_are_unavailable_without_netlogo() {
        let dir = tempfile::tempdir().unwrap();
        let roots = roots_with(dir.path(), None);
        let m = manifest(&format!(
            "[[model]]\nid = \"m\"\nset = \"C1\"\nsource = \"netlogo-install\"\npath = \"models/x.nlogox\"\nnetlogo = \"7.0.4\"\nsha256 = \"{HASH_A}\"\nlicense = \"MIT\"\n"
        ))
        .unwrap();
        assert!(matches!(
            check_model(&roots, &m.models[0]).unwrap(),
            Status::Unavailable(_)
        ));
    }

    #[test]
    fn collection_lists_matching_files_sorted() {
        let install = tempfile::tempdir().unwrap();
        let models = install.path().join("models");
        std::fs::create_dir_all(models.join("Sub Folder")).unwrap();
        std::fs::write(models.join("b.nlogox"), b"").unwrap();
        std::fs::write(models.join("Sub Folder/a.nlogox"), b"").unwrap();
        std::fs::write(models.join("c.png"), b"").unwrap();
        let roots = roots_with(Path::new("unused"), Some(install.path().to_path_buf()));
        let m = manifest(
            "[[collection]]\nid = \"lib\"\nset = \"C1\"\nsource = \"netlogo-install\"\nroot = \"models\"\nextensions = [\"nlogox\"]\nnetlogo = \"7.0.4\"\nlicense = \"various\"\n",
        )
        .unwrap();
        let files = roots.collection_files(&m.collections[0]).unwrap();
        let names: Vec<_> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_owned())
            .collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"a.nlogox".to_owned()));
        assert!(names.contains(&"b.nlogox".to_owned()));
    }

    #[test]
    fn sha256_matches_known_value() {
        // SHA-256 of the empty input.
        assert_eq!(
            sha256_reader(&b""[..]).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    /// The real manifest must always parse, and every file committed to the
    /// repository must match its recorded hash.
    #[test]
    fn repository_manifest_is_valid_and_repo_files_match() {
        let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
        let m = Manifest::load(&corpus.join("manifest.toml")).unwrap();
        let roots = roots_with(&corpus, None);
        for entry in m.models.iter().filter(|e| e.source == Source::Repo) {
            assert_eq!(
                check_model(&roots, entry).unwrap(),
                Status::Ok,
                "corpus file for `{}` does not match its hash",
                entry.id
            );
        }
    }
}
