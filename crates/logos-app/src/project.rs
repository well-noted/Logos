//! Projects (design 3.9) and secrets (design 3.7) for the open model, over `logos-project`
//! and `logos-secrets`.
//!
//! Secret values never reach the front end except when the user unmasks one, and never reach
//! a file or a log (Part K). Every set, delete, unmask and copy is logged by name and time in
//! `secrets-access.log`, as the CLI does.

use std::path::{Path, PathBuf};
use std::process::Command;

use logos_project::check::{Environment, Severity, check};
use logos_project::detect::{Reason, detect};
use logos_project::{FileEntry, PROJECT_FILE, Project, Python, Role, Secrets};
use logos_secrets::{Scope, SecretStore, SecretValue, SecretsError};
use serde::{Deserialize, Serialize};

use crate::session::{AppError, AppResult};

fn err(e: impl std::fmt::Display) -> AppError {
    AppError::new(e.to_string())
}

fn role_name(r: Role) -> &'static str {
    r.as_str()
}

fn parse_role(s: &str) -> Option<Role> {
    match s {
        "input" => Some(Role::Input),
        "output" => Some(Role::Output),
        "code" => Some(Role::Code),
        "config" => Some(Role::Config),
        _ => None,
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    pub path: String,
    pub role: String,
    pub description: Option<String>,
    pub dictionary: Option<String>,
    pub exists: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub dir: PathBuf,
    pub is_project: bool,
    pub model: String,
    pub engine: Option<String>,
    pub files: Vec<FileView>,
    pub required: Vec<String>,
    pub optional: Vec<String>,
    pub requirements: Option<String>,
    pub interpreter: Option<String>,
    /// `logos.toml` as written, when there is one.
    pub source: Option<String>,
}

fn model_name(model: &Path) -> AppResult<(PathBuf, String)> {
    let dir = model
        .parent()
        .ok_or_else(|| AppError::new("the model has no folder"))?
        .to_path_buf();
    let name = model
        .file_name()
        .ok_or_else(|| AppError::new("the model has no file name"))?
        .to_string_lossy()
        .into_owned();
    Ok((dir, name))
}

pub fn status(model: &Path) -> AppResult<ProjectView> {
    let (dir, name) = model_name(model)?;
    let toml = dir.join(PROJECT_FILE);
    if !toml.is_file() {
        return Ok(ProjectView {
            dir,
            is_project: false,
            model: name,
            engine: None,
            files: Vec::new(),
            required: Vec::new(),
            optional: Vec::new(),
            requirements: None,
            interpreter: None,
            source: None,
        });
    }
    let p = Project::open(&dir).map_err(err)?;
    let files = p
        .files()
        .map_err(err)?
        .into_iter()
        .map(|f| FileView {
            exists: dir.join(&f.path).exists(),
            path: f.path,
            role: role_name(f.role).into(),
            description: f.description,
            dictionary: f.dictionary,
        })
        .collect();
    let secrets = p.secrets().map_err(err)?;
    let python = p.python().map_err(err)?.unwrap_or_default();
    Ok(ProjectView {
        is_project: true,
        model: p.model().unwrap_or(name),
        engine: p.engine(),
        files,
        required: secrets.required,
        optional: secrets.optional,
        requirements: python.requirements,
        interpreter: python.interpreter,
        source: std::fs::read_to_string(&toml).ok(),
        dir,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalView {
    pub path: String,
    pub role: Option<String>,
    pub language: Option<String>,
    /// Why it was proposed: `file-open on line 212`, `found in the folder`.
    pub reasons: Vec<String>,
    pub exists: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectionView {
    pub model: String,
    pub files: Vec<ProposalView>,
    /// File names the model builds at run time: (primitive, line).
    pub dynamic: Vec<(String, usize)>,
    pub secret_names: Vec<String>,
    pub requirements: Option<String>,
    /// The Python interpreter found, with its version, when the model uses `py`.
    pub python: Option<String>,
    pub skipped_folders: Vec<(String, usize)>,
}

fn line_of(code: &str, offset: usize) -> usize {
    code[..offset.min(code.len())].matches('\n').count() + 1
}

/// What `logos project init` would propose for the model's folder (design 3.9).
pub fn detect_for(model: &Path, code: &str) -> AppResult<DetectionView> {
    let (dir, name) = model_name(model)?;
    let d = detect(&dir, &name, code);
    let files = d
        .files
        .iter()
        .map(|p| ProposalView {
            path: p.path.clone(),
            role: p.role.map(|r| role_name(r).into()),
            language: p.language.clone(),
            reasons: p
                .reasons
                .iter()
                .map(|r| match r {
                    Reason::Code { primitive, span } => {
                        format!("{primitive} on line {}", line_of(code, span.start))
                    }
                    Reason::PythonImport { module } => format!("py:run imports {module}"),
                    Reason::Experiments => "BehaviorSpace experiments file".into(),
                    Reason::Requirements => "Python requirements".into(),
                    Reason::EnvTemplate => "environment variable template".into(),
                    Reason::InFolder => "found in the folder".into(),
                })
                .collect(),
            exists: p.exists,
        })
        .collect();
    let uses_py = code.contains("py:");
    let python = if uses_py {
        ["python3", "python"].iter().find_map(|i| {
            let out = Command::new(i).arg("--version").output().ok()?;
            out.status.success().then(|| {
                let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
                format!("{i} ({v})")
            })
        })
    } else {
        None
    };
    Ok(DetectionView {
        model: name,
        files,
        dynamic: d
            .dynamic
            .iter()
            .map(|f| (f.primitive.clone(), line_of(code, f.span.start)))
            .collect(),
        secret_names: d.secret_names.into_iter().collect(),
        requirements: d.requirements,
        python,
        skipped_folders: d.skipped_folders,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitChoice {
    pub files: Vec<(String, String, Option<String>)>,
    pub required: Vec<String>,
    pub optional: Vec<String>,
    pub requirements: Option<String>,
}

/// Writes `logos.toml` with what the user confirmed in the setup review.
pub fn init(model: &Path, choice: &InitChoice) -> AppResult<ProjectView> {
    let (dir, name) = model_name(model)?;
    if dir.join(PROJECT_FILE).exists() {
        return Err(AppError::new(format!(
            "{} already exists in {}",
            PROJECT_FILE,
            dir.display()
        )));
    }
    let mut p = Project::new(&dir, &name).map_err(err)?;
    p.set_engine(&format!("netlogo-{}", crate::engine::NETLOGO_VERSION));
    for (path, role, language) in &choice.files {
        let Some(role) = parse_role(role) else {
            continue;
        };
        let mut f = FileEntry::new(path, role);
        f.language = language.clone();
        p.set_file(&f).map_err(err)?;
    }
    if choice.requirements.is_some() {
        p.set_python(&Python {
            requirements: choice.requirements.clone(),
            interpreter: None,
        });
    }
    if !choice.required.is_empty() || !choice.optional.is_empty() {
        p.set_secrets(&Secrets {
            required: choice.required.clone(),
            optional: choice.optional.clone(),
        })
        .map_err(err)?;
    }
    p.save().map_err(err)?;
    status(model)
}

/// Sets one file's role in `logos.toml`, or removes it with role `none`.
pub fn set_file_role(model: &Path, path: &str, role: &str) -> AppResult<ProjectView> {
    let (dir, _) = model_name(model)?;
    let mut p = Project::open(&dir).map_err(err)?;
    match parse_role(role) {
        Some(r) => {
            let mut f = p
                .files()
                .map_err(err)?
                .into_iter()
                .find(|f| f.path == path)
                .unwrap_or_else(|| FileEntry::new(path, r));
            f.role = r;
            p.set_file(&f).map_err(err)?;
        }
        None => {
            p.remove_file(path);
        }
    }
    p.save().map_err(err)?;
    status(model)
}

// ------------------------------------------------------------------------------- the check

struct Machine {
    project_id: Option<String>,
    passphrase: Option<String>,
}

impl Environment for Machine {
    fn run_python(&self, interpreter: &str, args: &[&str]) -> Option<String> {
        // Arguments as a list (D-17).
        let out = Command::new(interpreter).args(args).output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn has_secret(&self, name: &str) -> bool {
        match open_store(self.passphrase.as_deref()) {
            Ok(store) => logos_secrets::resolve(
                store.as_ref(),
                self.project_id.as_deref(),
                &[name.to_owned()],
            )
            .is_ok_and(|r| r.contains_key(name)),
            Err(_) => std::env::var_os(name).is_some_and(|v| !v.is_empty()),
        }
    }

    fn netlogo_home(&self, version: &str) -> Option<PathBuf> {
        logos_bridge_client::find_netlogo_home(version)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindingView {
    pub severity: String,
    pub subject: String,
    pub message: String,
}

/// `logos check` for the model's project (P3).
pub fn check_project(model: &Path, passphrase: Option<&str>) -> AppResult<Vec<FindingView>> {
    let (dir, _) = model_name(model)?;
    if !dir.join(PROJECT_FILE).is_file() {
        return Ok(Vec::new());
    }
    let p = Project::open(&dir).map_err(err)?;
    let env = Machine {
        project_id: p.id(),
        passphrase: passphrase.map(str::to_owned),
    };
    Ok(check(&p, &env)
        .into_iter()
        .map(|f| FindingView {
            severity: match f.severity {
                Severity::Ok => "ok",
                Severity::Warning => "warning",
                Severity::Error => "error",
            }
            .into(),
            subject: f.subject,
            message: f.message,
        })
        .collect())
}

// --------------------------------------------------------------------------------- secrets

/// The credential store. Where the operating system has none, the encrypted file needs the
/// passphrase the user typed in this session.
pub fn open_store(passphrase: Option<&str>) -> Result<Box<dyn SecretStore>, SecretsError> {
    let pass = passphrase.map(str::to_owned);
    logos_secrets::open_store(&move |_new| {
        pass.clone()
            .ok_or_else(|| SecretsError::File("needs-passphrase".into()))
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretRow {
    pub name: String,
    pub required: bool,
    /// Declared in `logos.toml`; false for names only stored.
    pub declared: bool,
    /// `project`, `user`, `environment`, or None when there is no value.
    pub source: Option<String>,
    pub masked: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretsView {
    pub store: String,
    pub needs_passphrase: bool,
    pub rows: Vec<SecretRow>,
    pub project_id: Option<String>,
}

fn project_scope(model: &Path, create: bool) -> AppResult<Option<Scope>> {
    let (dir, _) = model_name(model)?;
    if !dir.join(PROJECT_FILE).is_file() {
        return Ok(None);
    }
    let mut p = Project::open(&dir).map_err(err)?;
    let id = if create {
        let (id, changed) = p.ensure_id().map_err(err)?;
        if changed {
            p.save().map_err(err)?;
        }
        Some(id)
    } else {
        p.id()
    };
    Ok(id.map(Scope::Project))
}

pub fn list(model: &Path, passphrase: Option<&str>) -> AppResult<SecretsView> {
    let view = status(model)?;
    let store = match open_store(passphrase) {
        Ok(s) => s,
        Err(SecretsError::File(m)) if m.contains("needs-passphrase") => {
            return Ok(SecretsView {
                store: "encrypted file".into(),
                needs_passphrase: true,
                rows: Vec::new(),
                project_id: None,
            });
        }
        Err(e) => return Err(err(e)),
    };
    let scope = project_scope(model, false)?;
    let mut names: Vec<(String, bool, bool)> = view
        .required
        .iter()
        .map(|n| (n.clone(), true, true))
        .chain(view.optional.iter().map(|n| (n.clone(), false, true)))
        .collect();
    let mut stored = store.names(&Scope::User).map_err(err)?;
    if let Some(s) = &scope {
        stored.extend(store.names(s).map_err(err)?);
    }
    for n in stored {
        if !names.iter().any(|(m, _, _)| *m == n) {
            names.push((n, false, false));
        }
    }
    let id = scope.as_ref().and_then(|s| match s {
        Scope::Project(id) => Some(id.clone()),
        Scope::User => None,
    });
    let all: Vec<String> = names.iter().map(|(n, _, _)| n.clone()).collect();
    let found = logos_secrets::resolve(store.as_ref(), id.as_deref(), &all).map_err(err)?;
    let rows = names
        .into_iter()
        .map(|(name, required, declared)| {
            let hit = found.get(&name);
            SecretRow {
                source: hit.map(|(_, s)| {
                    match s {
                        logos_secrets::Source::Project => "project",
                        logos_secrets::Source::User => "user",
                        logos_secrets::Source::Environment => "environment",
                    }
                    .into()
                }),
                masked: hit.map(|(v, _)| v.masked()),
                name,
                required,
                declared,
            }
        })
        .collect();
    Ok(SecretsView {
        store: store.describe(),
        needs_passphrase: false,
        rows,
        project_id: id,
    })
}

fn scope_for(model: &Path, scope: &str) -> AppResult<Scope> {
    match scope {
        "project" => project_scope(model, true)?.ok_or_else(|| {
            AppError::new("Make this folder a project to keep secrets for it alone.")
        }),
        _ => Ok(Scope::User),
    }
}

pub fn set(
    model: &Path,
    passphrase: Option<&str>,
    name: &str,
    value: &str,
    scope: &str,
) -> AppResult<()> {
    logos_secrets::check_name(name).map_err(err)?;
    if value.is_empty() {
        return Err(AppError::new("The value is empty."));
    }
    let scope = scope_for(model, scope)?;
    let mut store = open_store(passphrase).map_err(err)?;
    store
        .set(&scope, name, &SecretValue::new(value))
        .map_err(err)?;
    let _ = logos_secrets::log_access("set", &scope, name);
    Ok(())
}

pub fn delete(model: &Path, passphrase: Option<&str>, name: &str, scope: &str) -> AppResult<bool> {
    let scope = scope_for(model, scope)?;
    let mut store = open_store(passphrase).map_err(err)?;
    let had = store.delete(&scope, name).map_err(err)?;
    let _ = logos_secrets::log_access("delete", &scope, name);
    Ok(had)
}

/// A value for the user to see (`unmask`) or copy (`copy`), after the front end confirmed
/// with the user. Logged by name and time.
pub fn reveal(
    model: &Path,
    passphrase: Option<&str>,
    name: &str,
    action: &str,
) -> AppResult<String> {
    let store = open_store(passphrase).map_err(err)?;
    let scope = project_scope(model, false)?;
    let id = scope.as_ref().and_then(|s| match s {
        Scope::Project(id) => Some(id.clone()),
        Scope::User => None,
    });
    let found =
        logos_secrets::resolve(store.as_ref(), id.as_deref(), &[name.to_string()]).map_err(err)?;
    let (value, source) = found
        .get(name)
        .ok_or_else(|| AppError::new(format!("{name} has no value")))?;
    let scope = match source {
        logos_secrets::Source::Project => scope.unwrap_or(Scope::User),
        _ => Scope::User,
    };
    let _ = logos_secrets::log_access(action, &scope, name);
    Ok(value.reveal().to_string())
}

/// The most recent unmasks and copies, by name and time, from `secrets-access.log`.
pub fn recent_access(limit: usize) -> Vec<String> {
    let Some(dir) = logos_secrets::config_dir() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(dir.join("secrets-access.log")) else {
        return Vec::new();
    };
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| l.contains("unmask") || l.contains("copy") || l.contains("show"))
        .map(str::to_owned)
        .collect();
    let keep = lines.len().saturating_sub(limit);
    lines.drain(..keep);
    lines.reverse();
    lines
}
