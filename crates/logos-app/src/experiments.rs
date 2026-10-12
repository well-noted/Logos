//! Experiments (design 3.3, 3.4): the model's BehaviorSpace experiments, the Logos-only
//! settings file (D-27), running an experiment into a results folder, and reading results.
//!
//! Running follows `logos run` (Phase 1, Part E) step for step: the same results folder
//! (`experiment.json`, one file per run with its run record, `table.csv`, `results.parquet`),
//! the same provenance, and resume. The experiment runs in its own bridge, started with the
//! project's secrets as environment variables, as `logos run` does (Part K).
//!
//! The orchestration repeats what `crates/logos-cli/src/run.rs` does; moving the shared part
//! into `logos-experiment` is a follow-up once both are tested against each other.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use logos_bridge_client::protocol::LabRunEnd;
use logos_bridge_client::{Bridge, BridgeConfig, EnvValue};
use logos_experiment::behaviorspace::parse_experiments;
use logos_experiment::provenance::{HashedFile, Provenance, RunRecord};
use logos_experiment::results::{ResultsDir, RunMeta};
use logos_experiment::runner::{ExperimentSource, RunOptions, run_experiment};
use logos_project::{PROJECT_FILE, Project, Role};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use toml_edit::{DocumentMut, Item, Table, value};

use crate::session::{AppError, AppResult, Emit};

fn err(e: impl std::fmt::Display) -> AppError {
    AppError::new(e.to_string())
}

// ------------------------------------------------------------------- the model's experiments

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentView {
    pub name: String,
    pub repetitions: u64,
    pub sequential_run_order: bool,
    pub run_metrics_every_step: bool,
    pub setup: String,
    pub go: String,
    pub time_limit: u64,
    pub exit_condition: String,
    pub metrics: Vec<String>,
    /// (name, values as NetLogo literals).
    pub variables: Vec<(String, Vec<String>)>,
    pub run_count: usize,
    pub settings: LogosSettings,
    /// The elements inside the `<experiment>`, in order; fields whose element is missing
    /// can't be edited yet.
    pub parts: Vec<String>,
    /// The value sets as written (stepped ones unexpanded), for the editor.
    pub value_sets: Vec<logos_nlogox::ValueSet>,
}

/// Experiments stored in the model, with their Logos-only settings.
pub fn list(model: &Path, model_text: &str) -> AppResult<Vec<ExperimentView>> {
    let file = SettingsFile::load(model);
    let nlogox = logos_nlogox::Nlogox::parse(model_text).ok();
    Ok(parse_experiments(model_text)
        .map_err(err)?
        .into_iter()
        .enumerate()
        .map(|(i, e)| ExperimentView {
            parts: nlogox
                .as_ref()
                .map(|n| n.experiment_parts(i))
                .unwrap_or_default(),
            value_sets: nlogox
                .as_ref()
                .map(|n| n.experiment_value_sets(i))
                .unwrap_or_default(),
            run_count: e.runs().len(),
            settings: file.get(&e.name),
            name: e.name,
            repetitions: e.repetitions,
            sequential_run_order: e.sequential_run_order,
            run_metrics_every_step: e.run_metrics_every_step,
            setup: e.setup,
            go: e.go,
            time_limit: e.time_limit,
            exit_condition: e.exit_condition,
            metrics: e.metrics,
            variables: e
                .variables
                .into_iter()
                .map(|v| (v.name, v.values))
                .collect(),
        })
        .collect())
}

// --------------------------------------------------------------- Logos-only settings (D-27)

/// Settings BehaviorSpace doesn't have, per experiment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LogosSettings {
    pub workers: usize,
    /// Off by default, as in headless BehaviorSpace.
    pub update_plots: bool,
    /// `parquet` and `table`; Parquet and NetLogo's table are always written for now.
    pub outputs: Vec<String>,
}

pub fn default_workers() -> usize {
    let n = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    (n * 3 / 4).max(1)
}

impl Default for LogosSettings {
    fn default() -> Self {
        LogosSettings {
            workers: default_workers(),
            update_plots: false,
            outputs: vec!["parquet".into(), "table".into()],
        }
    }
}

/// `<model name>.experiments.toml` next to the model. Unknown keys are kept.
pub struct SettingsFile {
    path: PathBuf,
    doc: DocumentMut,
}

pub const SETTINGS_FORMAT: i64 = 1;

pub fn settings_path(model: &Path) -> PathBuf {
    let stem = model
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "model".into());
    model.with_file_name(format!("{stem}.experiments.toml"))
}

impl SettingsFile {
    pub fn load(model: &Path) -> SettingsFile {
        let path = settings_path(model);
        let doc = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| t.parse::<DocumentMut>().ok())
            .unwrap_or_default();
        SettingsFile { path, doc }
    }

    pub fn get(&self, experiment: &str) -> LogosSettings {
        let mut s = LogosSettings::default();
        let Some(t) = self
            .doc
            .get("experiments")
            .and_then(|e| e.get(experiment))
            .and_then(Item::as_table_like)
        else {
            return s;
        };
        if let Some(w) = t.get("workers").and_then(Item::as_integer)
            && w > 0
        {
            s.workers = w as usize;
        }
        if let Some(b) = t.get("update_plots").and_then(Item::as_bool) {
            s.update_plots = b;
        }
        if let Some(a) = t.get("outputs").and_then(Item::as_array) {
            s.outputs = a
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect();
        }
        s
    }

    pub fn set(&mut self, experiment: &str, s: &LogosSettings) {
        if !self.doc.contains_key("format") {
            self.doc["format"] = value(SETTINGS_FORMAT);
        }
        if !self.doc.contains_key("experiments") {
            let mut t = Table::new();
            t.set_implicit(true);
            self.doc["experiments"] = Item::Table(t);
        }
        let exps = self.doc["experiments"].as_table_mut().expect("a table");
        if !exps.contains_key(experiment) {
            exps.insert(experiment, Item::Table(Table::new()));
        }
        let t = exps[experiment].as_table_mut().expect("a table");
        t["workers"] = value(s.workers as i64);
        t["update_plots"] = value(s.update_plots);
        let mut a = toml_edit::Array::new();
        for o in &s.outputs {
            a.push(o.as_str());
        }
        t["outputs"] = value(a);
    }

    pub fn save(&self) -> std::io::Result<()> {
        crate::settings::write_atomically(&self.path, self.doc.to_string().as_bytes())
    }
}

/// Saves an experiment's Logos-only settings; lists the file in `logos.toml` when the folder
/// is a project.
pub fn save_settings(model: &Path, experiment: &str, s: &LogosSettings) -> AppResult<()> {
    let mut f = SettingsFile::load(model);
    f.set(experiment, s);
    f.save().map_err(err)?;
    if let Some(dir) = model.parent()
        && dir.join(PROJECT_FILE).is_file()
        && let Ok(mut p) = Project::open(dir)
    {
        let name = settings_path(model)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if p.files().is_ok_and(|fs| !fs.iter().any(|f| f.path == name)) {
            let _ = p.set_file(&logos_project::FileEntry::new(&name, Role::Config));
            let _ = p.save();
        }
    }
    Ok(())
}

// ------------------------------------------------------------------------------ running

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRow {
    pub run: u64,
    pub settings: Vec<(String, String)>,
    /// `running`, `done`, `failed`, `stopped` or `waiting`.
    pub status: String,
    pub steps: u64,
    pub seconds: f64,
    pub worker: Option<usize>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StudyView {
    pub id: u64,
    pub name: String,
    pub out_dir: PathBuf,
    /// `starting`, `running`, `stopping`, `finished`, `stopped` or `failed`.
    pub state: String,
    pub total: usize,
    pub done: usize,
    pub failed: usize,
    pub workers: usize,
    pub elapsed: f64,
    pub runs: Vec<RunRow>,
    pub error: Option<String>,
    pub parquet: Option<PathBuf>,
    pub resumed: usize,
}

struct Study {
    view: Mutex<StudyView>,
    cancel: AtomicBool,
    started: Instant,
}

#[derive(Default)]
pub struct Studies {
    next: AtomicU64,
    all: Mutex<BTreeMap<u64, Arc<Study>>>,
}

fn recorded_path(project_dir: &Path, path: &Path) -> String {
    match path.strip_prefix(project_dir) {
        Ok(rel) => rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

/// The engine and the files the runs depend on, as `logos run` records them (E3).
fn provenance(
    project: Option<&Project>,
    project_dir: &Path,
    model: &Path,
    hello: &logos_bridge_client::protocol::HelloResult,
    netlogo_version: &str,
    secrets: Vec<String>,
) -> AppResult<Provenance> {
    let mut files = vec![HashedFile::new(
        &recorded_path(project_dir, model),
        "model",
        model,
    )];
    let toml = project_dir.join(PROJECT_FILE);
    if toml.is_file() {
        files.push(HashedFile::new(PROJECT_FILE, "project", &toml));
    }
    if let Some(p) = project {
        for f in p.files().map_err(err)? {
            if f.role == Role::Output {
                continue;
            }
            files.push(HashedFile::new(
                &f.path,
                f.role.as_str(),
                &project_dir.join(&f.path),
            ));
            if let Some(d) = &f.dictionary {
                files.push(HashedFile::new(d, "dictionary", &project_dir.join(d)));
            }
        }
    }
    files.dedup_by(|a, b| a.path == b.path);
    Ok(Provenance {
        logos_version: env!("CARGO_PKG_VERSION").to_owned(),
        netlogo_version: netlogo_version.to_owned(),
        bridge_version: hello.bridge_version.clone(),
        java_version: hello.java_version.clone(),
        java_vendor: hello.java_vendor.clone(),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        files,
        secrets,
    })
}

/// A folder name for an experiment: letters, digits, `-` and `_`.
fn folder_name(experiment: &str) -> String {
    let s: String = experiment
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() { "experiment".into() } else { s }
}

/// The values of the project's secrets for the bridge. Fails if a required one has none.
fn secret_env(
    project: Option<&Project>,
    passphrase: Option<&str>,
) -> AppResult<Vec<(String, EnvValue)>> {
    let Some(project) = project else {
        return Ok(Vec::new());
    };
    let declared = project.secrets().map_err(err)?;
    let names: Vec<String> = declared
        .required
        .iter()
        .chain(&declared.optional)
        .cloned()
        .collect();
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let store = crate::project::open_store(passphrase).map_err(err)?;
    let found =
        logos_secrets::resolve(store.as_ref(), project.id().as_deref(), &names).map_err(err)?;
    if let Some(missing) = declared.required.iter().find(|n| !found.contains_key(*n)) {
        return Err(AppError::new(format!(
            "The required secret {missing} has no value. Set it in Secrets first."
        )));
    }
    Ok(found
        .into_iter()
        .map(|(n, (v, _))| (n, EnvValue(v.reveal().to_owned())))
        .collect())
}

pub struct StartRequest {
    pub model: PathBuf,
    pub experiment: String,
    pub settings: LogosSettings,
    /// A results folder to resume, instead of starting a new one.
    pub resume: Option<PathBuf>,
    pub config: BridgeConfig,
    pub passphrase: Option<String>,
}

impl Studies {
    pub fn get(&self, id: u64) -> AppResult<StudyView> {
        let all = self.all.lock().unwrap();
        let s = all
            .get(&id)
            .ok_or_else(|| AppError::new(format!("no experiment run {id}")))?;
        let mut v = s.view.lock().unwrap().clone();
        if matches!(v.state.as_str(), "running" | "starting" | "stopping") {
            v.elapsed = s.started.elapsed().as_secs_f64();
        }
        Ok(v)
    }

    pub fn stop(&self, id: u64) -> AppResult<()> {
        let all = self.all.lock().unwrap();
        let s = all
            .get(&id)
            .ok_or_else(|| AppError::new(format!("no experiment run {id}")))?;
        s.cancel.store(true, Ordering::SeqCst);
        let mut v = s.view.lock().unwrap();
        if v.state == "running" {
            v.state = "stopping".into();
        }
        Ok(())
    }

    pub fn start(self: &Arc<Self>, req: StartRequest, emit: Emit) -> AppResult<u64> {
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let project_dir = req
            .model
            .parent()
            .ok_or_else(|| AppError::new("the model has no folder"))?
            .to_path_buf();
        let out_dir = match &req.resume {
            Some(d) => d.clone(),
            None => project_dir.join("results").join(format!(
                "{}-{}",
                folder_name(&req.experiment),
                chrono::Local::now().format("%Y%m%d-%H%M%S")
            )),
        };
        let study = Arc::new(Study {
            view: Mutex::new(StudyView {
                id,
                name: req.experiment.clone(),
                out_dir: out_dir.clone(),
                state: "starting".into(),
                total: 0,
                done: 0,
                failed: 0,
                workers: req.settings.workers,
                elapsed: 0.0,
                runs: Vec::new(),
                error: None,
                parquet: None,
                resumed: 0,
            }),
            cancel: AtomicBool::new(false),
            started: Instant::now(),
        });
        self.all.lock().unwrap().insert(id, study.clone());
        std::thread::Builder::new()
            .name(format!("logos-study-{id}"))
            .spawn(move || {
                let notify = |s: &Study| {
                    let v = s.view.lock().unwrap().clone();
                    emit(
                        "study",
                        json!({ "id": v.id, "state": v.state, "done": v.done, "total": v.total }),
                    );
                };
                let result = run_study(&study, req, &project_dir, &out_dir, &notify);
                {
                    let mut v = study.view.lock().unwrap();
                    v.elapsed = study.started.elapsed().as_secs_f64();
                    match result {
                        Ok(state) => v.state = state,
                        Err(e) => {
                            v.state = "failed".into();
                            v.error = Some(e.message);
                        }
                    }
                    for r in v.runs.iter_mut().filter(|r| r.status == "running") {
                        r.status = "waiting".into();
                    }
                }
                notify(&study);
            })
            .map_err(err)?;
        Ok(id)
    }
}

/// Runs the experiment; returns the final state (`finished` or `stopped`).
fn run_study(
    study: &Study,
    req: StartRequest,
    project_dir: &Path,
    out_dir: &Path,
    notify: &(dyn Fn(&Study) + Sync),
) -> AppResult<String> {
    let project = if project_dir.join(PROJECT_FILE).is_file() {
        Some(Project::open(project_dir).map_err(err)?)
    } else {
        None
    };
    let resume = match &req.resume {
        Some(d) => Some(ResultsDir::open(d).map_err(err)?),
        None => None,
    };
    let saved = match &resume {
        Some(r) => r.saved_runs().map_err(err)?,
        None => Vec::new(),
    };
    let mut config = req.config.clone();
    config.env = secret_env(project.as_ref(), req.passphrase.as_deref())?;
    let secret_names: Vec<String> = config.env.iter().map(|(n, _)| n.clone()).collect();
    let bridge = Bridge::spawn(&config)
        .map_err(|e| AppError::new(format!("the bridge didn't start: {e}")))?;
    let netlogo_version = bridge
        .hello()
        .netlogo_version
        .clone()
        .unwrap_or_else(|| config.netlogo_version.clone());
    let provenance = provenance(
        project.as_ref(),
        project_dir,
        &req.model,
        bridge.hello(),
        &netlogo_version,
        secret_names,
    )?;
    if let Some(then) = resume.as_ref().and_then(|r| r.meta.provenance.as_ref()) {
        let changed = then.differences(&provenance);
        if !changed.is_empty() {
            let _ = bridge.shutdown();
            return Err(AppError::new(format!(
                "These results were started with different inputs, so new runs couldn't be compared with them: {}. Start a new run instead.",
                changed.join("; ")
            )));
        }
    }
    let source = ExperimentSource {
        model: req.model.clone(),
        name: req.experiment.clone(),
        file: None,
    };
    let options = RunOptions {
        workers: req.settings.workers.max(1),
        update_plots: req.settings.update_plots,
        skip: saved.iter().map(|r| r.run_number).collect(),
        cancel: Some(&study.cancel),
        ..RunOptions::default()
    };
    struct Output {
        results: ResultsDir,
        table: logos_experiment::table::TableWriter<std::io::BufWriter<std::fs::File>>,
    }
    let output: Mutex<Option<Output>> = Mutex::new(None);
    let failure: Mutex<Option<String>> = Mutex::new(None);
    let fail = |e: String| {
        failure.lock().unwrap().get_or_insert(e);
        study.cancel.store(true, Ordering::SeqCst);
    };
    let resume_meta = resume.as_ref().map(|r| r.meta.clone());
    let update_plots = req.settings.update_plots;

    // Steps of the runs in progress come from the bridge's experiment.progress notifications
    // (G11), so the table moves while the runs go on.
    let finished = AtomicBool::new(false);
    let outcome = std::thread::scope(|scope| {
        scope.spawn(|| watch_progress(&bridge, study, &finished, notify));
        let outcome = run_experiment(
            &bridge,
            &source,
            &options,
            &|e, runs| {
                let setup = || -> Result<Output, String> {
                    let results = match &resume_meta {
                        Some(meta) => {
                            meta.check(e).map_err(|e| e.to_string())?;
                            ResultsDir::open(out_dir).map_err(|e| e.to_string())?
                        }
                        None => ResultsDir::create(
                            out_dir,
                            RunMeta {
                                experiment: e.name.clone(),
                                model: req.model.clone(),
                                project: project_dir.to_path_buf(),
                                setup_file: None,
                                netlogo_version: netlogo_version.clone(),
                                run_count: e.runs.len() as u64,
                                variables: e.variables.clone(),
                                metrics: e.metrics.clone(),
                                update_plots,
                                created: chrono::Local::now().to_rfc3339(),
                                provenance: Some(provenance.clone()),
                            },
                        )
                        .map_err(|e| e.to_string())?,
                    };
                    let table = results.start_table(e, &saved).map_err(|e| e.to_string())?;
                    Ok(Output { results, table })
                };
                match setup() {
                    Ok(o) => *output.lock().unwrap() = Some(o),
                    Err(msg) => fail(msg),
                }
                let mut v = study.view.lock().unwrap();
                v.state = "running".into();
                v.total = e.runs.len();
                v.resumed = saved.len();
                v.done = saved.len();
                v.workers = options.workers.min(runs.len()).max(1);
                v.runs = e
                    .runs
                    .iter()
                    .enumerate()
                    .map(|(i, settings)| {
                        let n = i as u64 + 1;
                        let prior = saved.iter().find(|r| r.run_number == n);
                        RunRow {
                            run: n,
                            settings: settings
                                .iter()
                                .map(|s| (s.name.clone(), s.cell.text.clone()))
                                .collect(),
                            status: if prior.is_some() {
                                "done".into()
                            } else {
                                "waiting".into()
                            },
                            steps: prior.map_or(0, |r| r.steps),
                            seconds: prior.map_or(0.0, |r| r.elapsed_millis / 1000.0),
                            worker: None,
                            errors: Vec::new(),
                        }
                    })
                    .collect();
                // The runner starts runs in order on its workers; mark the first ones running.
                let workers = v.workers;
                for r in v
                    .runs
                    .iter_mut()
                    .filter(|r| r.status == "waiting")
                    .take(workers)
                {
                    r.status = "running".into();
                }
                drop(v);
                notify(study);
            },
            &|_, o| {
                let r = &o.result;
                let halted = r.reason == LabRunEnd::Halted;
                if !halted && let Some(out) = output.lock().unwrap().as_mut() {
                    let ended = chrono::Local::now();
                    let started =
                        ended - chrono::Duration::milliseconds(r.elapsed_millis.round() as i64);
                    let record = RunRecord {
                        run_number: r.run_number,
                        started: started.to_rfc3339_opts(chrono::SecondsFormat::Millis, false),
                        ended: ended.to_rfc3339_opts(chrono::SecondsFormat::Millis, false),
                        worker: o.worker,
                        random_seed: r
                            .settings
                            .iter()
                            .find(|s| s.name.eq_ignore_ascii_case("random-seed"))
                            .map(|s| s.cell.text.clone()),
                        provenance: provenance.clone(),
                    };
                    let saved = out.results.save_run(r, &record).map_err(|e| e.to_string());
                    let written = out.table.write_run(r).map_err(|e| e.to_string());
                    if let Err(e) = saved.and(written) {
                        fail(e);
                    }
                }
                let mut v = study.view.lock().unwrap();
                if let Some(row) = v.runs.iter_mut().find(|x| x.run == r.run_number) {
                    row.status = if halted {
                        "stopped".into()
                    } else if r.reason == LabRunEnd::Failed {
                        "failed".into()
                    } else {
                        "done".into()
                    };
                    row.steps = r.steps;
                    row.seconds = r.elapsed_millis / 1000.0;
                    row.worker = Some(o.worker);
                    row.errors = r
                        .errors
                        .iter()
                        .map(|e| format!("{}: {}", e.kind, e.message.lines().next().unwrap_or("")))
                        .collect();
                }
                if !halted {
                    v.done += 1;
                    if r.reason == LabRunEnd::Failed || !r.errors.is_empty() {
                        v.failed += 1;
                    }
                }
                // The next waiting run is now on a worker, unless the run is being stopped.
                if !study.cancel.load(Ordering::SeqCst)
                    && let Some(next) = v.runs.iter_mut().find(|x| x.status == "waiting")
                {
                    next.status = "running".into();
                }
                drop(v);
                notify(study);
            },
        );
        finished.store(true, Ordering::SeqCst);
        outcome
    });
    let _ = bridge.shutdown();
    let outcome = outcome.map_err(|e| AppError::new(e.to_string()))?;
    if let Some(e) = failure.into_inner().unwrap() {
        return Err(AppError::new(e));
    }
    let Some(out) = output.into_inner().unwrap() else {
        return Err(AppError::new("the results folder was not set up"));
    };
    drop(out.table);
    let saved_now = out.results.saved_runs().map_err(err)?;
    let mut extra = Vec::new();
    for (when, e) in [
        ("pre-experiment", &outcome.pre_experiment_error),
        ("post-experiment", &outcome.post_experiment_error),
    ] {
        if let Some(e) = e {
            extra.push(format!(
                "Error in the {when} commands: {}: {}",
                e.kind, e.message
            ));
        }
    }
    if !extra.is_empty() {
        study.view.lock().unwrap().error = Some(extra.join("\n"));
    }
    if saved_now.len() < outcome.experiment.runs.len() {
        return Ok("stopped".into());
    }
    let parquet = out
        .results
        .write_parquet(&outcome.experiment)
        .map_err(err)?;
    study.view.lock().unwrap().parquet = Some(parquet);
    Ok("finished".into())
}

/// Shows each run's steps from `experiment.progress` until `finished` is set.
fn watch_progress(
    bridge: &Bridge,
    study: &Study,
    finished: &AtomicBool,
    notify: &(dyn Fn(&Study) + Sync),
) {
    let mut last_notify = std::time::Instant::now();
    while !finished.load(Ordering::SeqCst) {
        let Some(n) = bridge.next_notification(std::time::Duration::from_millis(200)) else {
            continue;
        };
        if n.method != "experiment.progress" {
            continue;
        }
        let Ok(p) = serde_json::from_value::<logos_protocol::ExperimentProgress>(n.params) else {
            continue;
        };
        {
            let mut v = study.view.lock().unwrap();
            if let Some(row) = v.runs.iter_mut().find(|r| r.run == p.run_number)
                && (row.status == "running" || row.status == "waiting")
            {
                row.status = "running".into();
                row.steps = p.steps;
                row.seconds = p.elapsed_millis / 1000.0;
            }
        }
        if last_notify.elapsed() >= std::time::Duration::from_millis(250) {
            last_notify = std::time::Instant::now();
            notify(study);
        }
    }
}

// ------------------------------------------------------------------------------- results

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PastRun {
    pub dir: PathBuf,
    pub experiment: String,
    pub created: String,
    pub run_count: u64,
    pub saved: usize,
    pub complete: bool,
}

/// Results folders under the project's `results/` folder, newest first.
pub fn past(model: &Path) -> Vec<PastRun> {
    let Some(dir) = model.parent().map(|d| d.join("results")) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<PastRun> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let r = ResultsDir::open(&e.path()).ok()?;
            let saved = r.saved_runs().map(|s| s.len()).unwrap_or(0);
            Some(PastRun {
                dir: e.path(),
                experiment: r.meta.experiment.clone(),
                created: r.meta.created.clone(),
                run_count: r.meta.run_count,
                saved,
                complete: saved as u64 >= r.meta.run_count
                    && e.path()
                        .join(logos_experiment::results::PARQUET_FILE)
                        .is_file(),
            })
        })
        .collect();
    out.sort_by(|a, b| b.created.cmp(&a.created));
    out
}

/// A results folder's table: column names and up to `limit` rows, values as text, and the
/// runs with their settings and records.
pub fn read_results(dir: &Path, limit: usize) -> AppResult<Json> {
    let r = ResultsDir::open(dir).map_err(err)?;
    let parquet = dir.join(logos_experiment::results::PARQUET_FILE);
    let (columns, rows, total) = if parquet.is_file() {
        let rb = logos_experiment::parquet::read_parquet(&parquet).map_err(err)?;
        let total = rb.rows.len();
        let rows: Vec<Vec<Json>> = rb
            .rows
            .iter()
            .take(limit)
            .map(|row| {
                row.iter()
                    .map(|v| match v {
                        None => Json::Null,
                        Some(logos_bridge_client::protocol::NlValue::Number(n)) => json!(n),
                        Some(v) => Json::String(crate::values::display(v, None)),
                    })
                    .collect()
            })
            .collect();
        (rb.columns, rows, total)
    } else {
        (Vec::new(), Vec::new(), 0)
    };
    let runs: Vec<Json> = r
        .saved()
        .map_err(err)?
        .into_iter()
        .map(|s| {
            json!({
                "run": s.result.run_number,
                "steps": s.result.steps,
                "seconds": s.result.elapsed_millis / 1000.0,
                "reason": format!("{:?}", s.result.reason),
                "settings": s.result.settings.iter().map(|x| (x.name.clone(), x.cell.text.clone())).collect::<Vec<_>>(),
                "errors": s.result.errors.iter().map(|e| format!("{}: {}", e.kind, e.message)).collect::<Vec<_>>(),
                "record": s.record,
            })
        })
        .collect();
    Ok(json!({
        "meta": r.meta,
        "columns": columns,
        "rows": rows,
        "totalRows": total,
        "runs": runs,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_file_round_trip_keeps_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("AA4 Combined.nlogox");
        assert_eq!(
            settings_path(&model),
            dir.path().join("AA4 Combined.experiments.toml")
        );
        std::fs::write(
            settings_path(&model),
            "format = 1\nfuture = \"keep\"\n\n[experiments.\"N3-limit1\"]\nworkers = 4\nbudget = 10\n",
        )
        .unwrap();
        let f = SettingsFile::load(&model);
        let s = f.get("N3-limit1");
        assert_eq!(s.workers, 4);
        assert!(!s.update_plots);
        save_settings(
            &model,
            "N3-limit1",
            &LogosSettings {
                workers: 8,
                update_plots: true,
                outputs: vec!["parquet".into()],
            },
        )
        .unwrap();
        save_settings(&model, "other", &LogosSettings::default()).unwrap();
        let text = std::fs::read_to_string(settings_path(&model)).unwrap();
        assert!(text.contains("future = \"keep\""), "{text}");
        assert!(text.contains("budget = 10"), "{text}");
        let f = SettingsFile::load(&model);
        assert_eq!(f.get("N3-limit1").workers, 8);
        assert!(f.get("N3-limit1").update_plots);
        assert_eq!(f.get("other"), LogosSettings::default());
        assert_eq!(f.get("missing"), LogosSettings::default());
    }

    #[test]
    fn folder_names() {
        assert_eq!(folder_name("N3 limit/1"), "N3_limit_1");
        assert_eq!(folder_name(""), "experiment");
    }

    #[test]
    fn lists_aa4_experiments() {
        let text = include_str!("../../../corpus/own/aa4-combined/AA4-Combined.nlogox");
        let dir = tempfile::tempdir().unwrap();
        let list = list(&dir.path().join("AA4-Combined.nlogox"), text).unwrap();
        assert_eq!(list.len(), 8);
        let n3 = list.iter().find(|e| e.name == "N3-limit1").unwrap();
        assert_eq!(n3.run_count, 60);
    }
}
