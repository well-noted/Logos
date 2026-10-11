//! Logos projects (Phase 1, Part P).
//!
//! A project is a folder with one NetLogo model and the files it uses. `logos.toml`, next to
//! the model, describes them. The model file is never changed to make it part of a project, so
//! the folder still opens in NetLogo (D-19).
//!
//! [`Project`] reads `logos.toml` into typed values and writes changes back through
//! `toml_edit`, so comments, formatting, key order and keys Logos doesn't know are kept.

pub mod dictionary;

use std::fmt;
use std::path::{Path, PathBuf};

use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, value};

pub use dictionary::{Column, DataDictionary};

/// The project file's name.
pub const PROJECT_FILE: &str = "logos.toml";

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot write {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{0} is not valid TOML: {1}")]
    Toml(PathBuf, String),
    #[error("{0}: {1}")]
    Invalid(String, String),
}

/// What a file is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Data the model reads.
    Input,
    /// Files the model or its experiments write.
    Output,
    /// Code the model runs: `.nls` includes, Python modules.
    Code,
    /// Settings files.
    Config,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Input => "input",
            Role::Output => "output",
            Role::Code => "code",
            Role::Config => "config",
        }
    }

    fn parse(s: &str) -> Option<Role> {
        Some(match s {
            "input" => Role::Input,
            "output" => Role::Output,
            "code" => Role::Code,
            "config" => Role::Config,
            _ => return None,
        })
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One `[[files]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Relative to the project folder, with `/` separators.
    pub path: String,
    pub role: Role,
    pub description: Option<String>,
    /// A data dictionary file for this file (see [`DataDictionary`]).
    pub dictionary: Option<String>,
    /// For code files, for example `python` or `netlogo`.
    pub language: Option<String>,
}

impl FileEntry {
    pub fn new(path: impl Into<String>, role: Role) -> FileEntry {
        FileEntry {
            path: path.into(),
            role,
            description: None,
            dictionary: None,
            language: None,
        }
    }
}

/// The `[python]` table, for models that use the `py` extension.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Python {
    /// A requirements file, relative to the project folder.
    pub requirements: Option<String>,
    /// A Python interpreter to use; `None` lets Logos find one.
    pub interpreter: Option<String>,
}

/// The `[secrets]` table: names only. Values live in the credential store (Part K).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Secrets {
    pub required: Vec<String>,
    pub optional: Vec<String>,
}

/// A project: its folder and its `logos.toml`.
#[derive(Debug, Clone)]
pub struct Project {
    dir: PathBuf,
    doc: DocumentMut,
}

impl Project {
    /// A new project for `model` (a file name in `dir`). Nothing is written until [`save`].
    ///
    /// [`save`]: Project::save
    pub fn new(dir: impl Into<PathBuf>, model: &str) -> Result<Project, ProjectError> {
        check_relative_path("model", model)?;
        let mut doc = DocumentMut::new();
        let mut project = Table::new();
        project.insert("model", value(model));
        doc.insert("project", Item::Table(project));
        Ok(Project {
            dir: dir.into(),
            doc,
        })
    }

    /// Reads `logos.toml` from `dir`.
    pub fn open(dir: impl Into<PathBuf>) -> Result<Project, ProjectError> {
        let dir = dir.into();
        let path = dir.join(PROJECT_FILE);
        let text = std::fs::read_to_string(&path).map_err(|source| ProjectError::Read {
            path: path.clone(),
            source,
        })?;
        Project::parse(dir, &text).map_err(|e| match e {
            ProjectError::Toml(_, m) => ProjectError::Toml(path, m),
            e => e,
        })
    }

    /// Parses `logos.toml` text for a project in `dir`, and checks it.
    pub fn parse(dir: impl Into<PathBuf>, text: &str) -> Result<Project, ProjectError> {
        let doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| {
            ProjectError::Toml(PROJECT_FILE.into(), e.to_string())
        })?;
        let p = Project {
            dir: dir.into(),
            doc,
        };
        p.model()?;
        p.files()?;
        p.python()?;
        p.secrets()?;
        Ok(p)
    }

    /// Writes `logos.toml` to the project folder.
    pub fn save(&self) -> Result<(), ProjectError> {
        let path = self.dir.join(PROJECT_FILE);
        std::fs::write(&path, self.to_toml()).map_err(|source| ProjectError::Write { path, source })
    }

    /// The `logos.toml` text, with everything Logos didn't change as it was.
    pub fn to_toml(&self) -> String {
        self.doc.to_string()
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The model file name, relative to the project folder.
    pub fn model(&self) -> Result<String, ProjectError> {
        let m = self
            .doc
            .get("project")
            .and_then(|p| p.get("model"))
            .ok_or_else(|| invalid("project.model", "missing"))?;
        let m = m
            .as_str()
            .ok_or_else(|| invalid("project.model", "must be a string"))?;
        check_relative_path("project.model", m)?;
        Ok(m.to_owned())
    }

    /// The model file's full path.
    pub fn model_path(&self) -> Result<PathBuf, ProjectError> {
        Ok(self.dir.join(self.model()?))
    }

    /// The engine the project was last run on, for example `netlogo-7.0.4`.
    pub fn engine(&self) -> Option<String> {
        self.doc
            .get("project")
            .and_then(|p| p.get("engine"))
            .and_then(Item::as_str)
            .map(str::to_owned)
    }

    pub fn set_engine(&mut self, engine: &str) {
        self.project_table()["engine"] = value(engine);
    }

    pub fn files(&self) -> Result<Vec<FileEntry>, ProjectError> {
        let Some(item) = self.doc.get("files") else {
            return Ok(Vec::new());
        };
        let tables = item
            .as_array_of_tables()
            .ok_or_else(|| invalid("files", "must be an array of tables ([[files]])"))?;
        tables
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let at = |key: &str| format!("files[{i}].{key}");
                let path = string(t, "path", &at("path"))?
                    .ok_or_else(|| invalid(&at("path"), "missing"))?;
                check_relative_path(&at("path"), &path)?;
                let role = string(t, "role", &at("role"))?
                    .ok_or_else(|| invalid(&at("role"), "missing"))?;
                let role = Role::parse(&role)
                    .ok_or_else(|| invalid(&at("role"), "must be input, output, code or config"))?;
                let dictionary = string(t, "dictionary", &at("dictionary"))?;
                if let Some(d) = &dictionary {
                    check_relative_path(&at("dictionary"), d)?;
                }
                Ok(FileEntry {
                    path,
                    role,
                    description: string(t, "description", &at("description"))?,
                    dictionary,
                    language: string(t, "language", &at("language"))?,
                })
            })
            .collect()
    }

    /// Adds a file, or updates the entry with the same path. Keys of an existing entry that
    /// Logos doesn't know are kept.
    pub fn set_file(&mut self, file: &FileEntry) -> Result<(), ProjectError> {
        check_relative_path("path", &file.path)?;
        let files = self
            .doc
            .entry("files")
            .or_insert(Item::ArrayOfTables(ArrayOfTables::new()))
            .as_array_of_tables_mut()
            .ok_or_else(|| invalid("files", "must be an array of tables ([[files]])"))?;
        let existing = files
            .iter()
            .position(|t| t.get("path").and_then(Item::as_str) == Some(file.path.as_str()));
        let table = match existing {
            Some(i) => files.get_mut(i).unwrap(),
            None => {
                files.push(Table::new());
                files.get_mut(files.len() - 1).unwrap()
            }
        };
        table["path"] = value(&file.path);
        table["role"] = value(file.role.as_str());
        set_optional(table, "description", file.description.as_deref());
        set_optional(table, "dictionary", file.dictionary.as_deref());
        set_optional(table, "language", file.language.as_deref());
        Ok(())
    }

    /// Removes the entry for `path`. Returns whether there was one.
    pub fn remove_file(&mut self, path: &str) -> bool {
        let Some(files) = self
            .doc
            .get_mut("files")
            .and_then(Item::as_array_of_tables_mut)
        else {
            return false;
        };
        let before = files.len();
        files.retain(|t| t.get("path").and_then(Item::as_str) != Some(path));
        before != files.len()
    }

    pub fn python(&self) -> Result<Option<Python>, ProjectError> {
        let Some(item) = self.doc.get("python") else {
            return Ok(None);
        };
        let t = item
            .as_table()
            .ok_or_else(|| invalid("python", "must be a table"))?;
        let empty_is_none = |s: Option<String>| s.filter(|s| !s.is_empty());
        Ok(Some(Python {
            requirements: empty_is_none(string(t, "requirements", "python.requirements")?),
            interpreter: empty_is_none(string(t, "interpreter", "python.interpreter")?),
        }))
    }

    pub fn set_python(&mut self, python: &Python) {
        let t = self
            .doc
            .entry("python")
            .or_insert(Item::Table(Table::new()))
            .as_table_mut()
            .expect("python is a table (checked when read)");
        set_optional(t, "requirements", python.requirements.as_deref());
        set_optional(t, "interpreter", python.interpreter.as_deref());
    }

    pub fn secrets(&self) -> Result<Secrets, ProjectError> {
        let Some(item) = self.doc.get("secrets") else {
            return Ok(Secrets::default());
        };
        let t = item
            .as_table()
            .ok_or_else(|| invalid("secrets", "must be a table"))?;
        Ok(Secrets {
            required: names(t, "required", "secrets.required")?,
            optional: names(t, "optional", "secrets.optional")?,
        })
    }

    pub fn set_secrets(&mut self, secrets: &Secrets) -> Result<(), ProjectError> {
        for n in secrets.required.iter().chain(&secrets.optional) {
            check_secret_name(n)?;
        }
        let t = self
            .doc
            .entry("secrets")
            .or_insert(Item::Table(Table::new()))
            .as_table_mut()
            .expect("secrets is a table (checked when read)");
        t["required"] = value(Array::from_iter(
            secrets.required.iter().map(String::as_str),
        ));
        t["optional"] = value(Array::from_iter(
            secrets.optional.iter().map(String::as_str),
        ));
        Ok(())
    }

    fn project_table(&mut self) -> &mut Table {
        self.doc
            .entry("project")
            .or_insert(Item::Table(Table::new()))
            .as_table_mut()
            .expect("project is a table")
    }
}

fn invalid(key: &str, message: &str) -> ProjectError {
    ProjectError::Invalid(key.to_owned(), message.to_owned())
}

fn string(t: &Table, key: &str, name: &str) -> Result<Option<String>, ProjectError> {
    match t.get(key) {
        None => Ok(None),
        Some(i) => i
            .as_str()
            .map(|s| Some(s.to_owned()))
            .ok_or_else(|| invalid(name, "must be a string")),
    }
}

fn names(t: &Table, key: &str, name: &str) -> Result<Vec<String>, ProjectError> {
    let Some(item) = t.get(key) else {
        return Ok(Vec::new());
    };
    let arr = item
        .as_array()
        .ok_or_else(|| invalid(name, "must be a list of names"))?;
    arr.iter()
        .map(|v| {
            let s = v
                .as_str()
                .ok_or_else(|| invalid(name, "must be a list of names"))?;
            check_secret_name(s)?;
            Ok(s.to_owned())
        })
        .collect()
}

fn set_optional(t: &mut Table, key: &str, v: Option<&str>) {
    match v {
        Some(v) => {
            t[key] = value(v);
        }
        None => {
            t.remove(key);
        }
    }
}

/// Paths in `logos.toml` are relative to the project folder, use `/`, and stay inside it.
fn check_relative_path(key: &str, p: &str) -> Result<(), ProjectError> {
    let bad = p.is_empty()
        || p.contains('\\')
        || p.starts_with('/')
        || p.split('/').any(|seg| seg == ".." || seg.is_empty())
        || (p.len() >= 2 && p.as_bytes()[1] == b':');
    if bad {
        return Err(invalid(
            key,
            &format!("`{p}` must be a relative path inside the project folder, with / separators"),
        ));
    }
    Ok(())
}

/// Secret names are environment variable names: letters, digits and `_`, not starting with a
/// digit.
fn check_secret_name(n: &str) -> Result<(), ProjectError> {
    let ok = !n.is_empty()
        && !n.as_bytes()[0].is_ascii_digit()
        && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    if !ok {
        return Err(invalid(
            "secrets",
            &format!("`{n}` is not an environment variable name"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"# AA4 project
[project]
model = "AA4-Combined.nlogox"
engine = "netlogo-7.0.4"
future-key = { kept = true }   # a key Logos doesn't know

[[files]]
path = "data/water.txt"
role = "input"
description = "Water sources by period"
dictionary = "data/water.dictionary.toml"
checksum-note = "unknown key, kept"

[[files]]
path = "mastoc_llm_bridge.py"
role = "code"
language = "python"

[python]
requirements = "requirements.txt"
interpreter = ""

[secrets]
required = ["ANTHROPIC_API_KEY"]
optional = ["OPENAI_API_KEY"]

[some-future-section]
x = 1
"#;

    fn project() -> Project {
        Project::parse("proj", EXAMPLE).unwrap()
    }

    #[test]
    fn reads_every_section() {
        let p = project();
        assert_eq!(p.model().unwrap(), "AA4-Combined.nlogox");
        assert_eq!(
            p.model_path().unwrap(),
            Path::new("proj/AA4-Combined.nlogox")
        );
        assert_eq!(p.engine().as_deref(), Some("netlogo-7.0.4"));
        let files = p.files().unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].role, Role::Input);
        assert_eq!(
            files[0].dictionary.as_deref(),
            Some("data/water.dictionary.toml")
        );
        assert_eq!(files[1].language.as_deref(), Some("python"));
        let py = p.python().unwrap().unwrap();
        assert_eq!(py.requirements.as_deref(), Some("requirements.txt"));
        assert_eq!(py.interpreter, None, "empty means let Logos find one");
        let s = p.secrets().unwrap();
        assert_eq!(s.required, ["ANTHROPIC_API_KEY"]);
        assert_eq!(s.optional, ["OPENAI_API_KEY"]);
    }

    #[test]
    fn unmodified_round_trip_is_identical() {
        assert_eq!(project().to_toml(), EXAMPLE);
    }

    #[test]
    fn edits_keep_comments_and_unknown_keys() {
        let mut p = project();
        p.set_engine("netlogo-7.0.5");
        let mut f = p.files().unwrap()[0].clone();
        f.description = Some("Water sources".into());
        p.set_file(&f).unwrap();
        p.set_file(&FileEntry::new("data/map.txt", Role::Input))
            .unwrap();
        assert!(p.remove_file("mastoc_llm_bridge.py"));
        assert!(!p.remove_file("nope.txt"));
        p.set_secrets(&Secrets {
            required: vec!["ANTHROPIC_API_KEY".into(), "OLLAMA_HOST".into()],
            optional: vec![],
        })
        .unwrap();
        let text = p.to_toml();
        for kept in [
            "# AA4 project",
            "future-key = { kept = true }   # a key Logos doesn't know",
            "checksum-note = \"unknown key, kept\"",
            "[some-future-section]\nx = 1",
        ] {
            assert!(text.contains(kept), "lost `{kept}`:\n{text}");
        }
        let q = Project::parse("proj", &text).unwrap();
        assert_eq!(q.engine().as_deref(), Some("netlogo-7.0.5"));
        let files = q.files().unwrap();
        assert_eq!(
            files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
            ["data/water.txt", "data/map.txt"]
        );
        assert_eq!(files[0].description.as_deref(), Some("Water sources"));
        assert_eq!(
            q.secrets().unwrap().required,
            ["ANTHROPIC_API_KEY", "OLLAMA_HOST"]
        );
    }

    #[test]
    fn new_project_writes_and_reads_back() {
        let dir = std::env::temp_dir().join(format!("logos project test {}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut p = Project::new(&dir, "Model One.nlogox").unwrap();
        p.set_file(&FileEntry::new("Data/in put.csv", Role::Input))
            .unwrap();
        p.set_python(&Python {
            requirements: Some("requirements.txt".into()),
            interpreter: None,
        });
        p.save().unwrap();
        let q = Project::open(&dir).unwrap();
        assert_eq!(q.model().unwrap(), "Model One.nlogox");
        assert_eq!(q.files().unwrap()[0].path, "Data/in put.csv");
        assert_eq!(
            q.python().unwrap().unwrap().requirements.as_deref(),
            Some("requirements.txt")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejects_bad_values() {
        let bad = |text: &str| Project::parse("p", text).unwrap_err().to_string();
        assert!(bad("").contains("project.model"));
        assert!(bad("[project]\nmodel = 3").contains("must be a string"));
        assert!(bad("[project]\nmodel = \"../m.nlogox\"").contains("relative path"));
        assert!(bad("[project]\nmodel = \"C:/m.nlogox\"").contains("relative path"));
        assert!(bad("[project]\nmodel = \"a\\\\m.nlogox\"").contains("relative path"));
        assert!(
            bad("[project]\nmodel = \"m.nlogox\"\n[[files]]\npath = \"x\"\nrole = \"data\"")
                .contains("must be input, output, code or config")
        );
        assert!(
            bad("[project]\nmodel = \"m.nlogox\"\n[secrets]\nrequired = [\"MY-KEY\"]")
                .contains("not an environment variable name")
        );
        assert!(bad("[project\n").contains("not valid TOML"));
    }
}
