//! `logos run`: run a BehaviorSpace experiment (Phase 1, Part E).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use logos_bridge_client::{Bridge, BridgeConfig, ClientError, EnvValue, find_netlogo_home};
use logos_experiment::provenance::{HashedFile, Provenance, RunRecord};
use logos_experiment::results::{ResultsDir, RunMeta};
use logos_experiment::runner::{ExperimentSource, RunOptions, RunnerError, run_experiment};
use logos_project::{PROJECT_FILE, Project, Role};

pub const USAGE: &str =
    "  logos run EXPERIMENT [FOLDER] [--workers N] [--setup-file FILE] [--out DIR]
            [--update-plots]
  logos run --resume DIR [--workers N]

  run            run a BehaviorSpace experiment of the model in FOLDER (default: the current
                 folder). Writes a results folder: table.csv (NetLogo's table output) as
                 runs finish, one file per finished run, and results.parquet at the end.
                 Ctrl-C stops the runs in progress after their current step; press it
                 again to quit at once.
    --workers N      runs at the same time (default: three quarters of the processors, as in
                     NetLogo)
    --setup-file F   read the experiment from a BehaviorSpace XML file instead of the model
    --out DIR        the results folder (default: FOLDER/results/EXPERIMENT-DATE-TIME)
    --update-plots   update plots during runs (off by default, as in headless NetLogo)
    --resume DIR     finish an interrupted experiment: run only the runs DIR doesn't have.
                     Refused if the engine, the model or an input file changed since";

struct Args {
    experiment: Option<String>,
    dir: PathBuf,
    workers: Option<usize>,
    setup_file: Option<PathBuf>,
    out: Option<PathBuf>,
    update_plots: bool,
    resume: Option<PathBuf>,
}

fn parse(args: &[&str]) -> Result<Args, String> {
    let mut experiment = None;
    let mut dir = None;
    let mut workers = None;
    let mut setup_file = None;
    let mut out = None;
    let mut update_plots = false;
    let mut resume = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .map(|s| s.to_string())
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match *a {
            "--workers" => {
                let n: usize = value("--workers")?
                    .parse()
                    .map_err(|_| "--workers needs a number".to_owned())?;
                if n == 0 {
                    return Err("--workers must be at least 1".into());
                }
                workers = Some(n);
            }
            "--setup-file" => setup_file = Some(PathBuf::from(value("--setup-file")?)),
            "--out" => out = Some(PathBuf::from(value("--out")?)),
            "--resume" => resume = Some(PathBuf::from(value("--resume")?)),
            "--update-plots" => update_plots = true,
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            s if experiment.is_none() => experiment = Some(s.to_owned()),
            s if dir.is_none() => dir = Some(PathBuf::from(s)),
            s => return Err(format!("unexpected argument {s}")),
        }
    }
    if resume.is_some() {
        if experiment.is_some() || setup_file.is_some() || out.is_some() || update_plots {
            return Err(
                "--resume takes the experiment and its settings from the results folder; \
                 only --workers can be given with it"
                    .into(),
            );
        }
    } else if experiment.is_none() {
        return Err("name the experiment to run".into());
    }
    Ok(Args {
        experiment,
        dir: dir.unwrap_or_else(|| PathBuf::from(".")),
        workers,
        setup_file,
        out,
        update_plots,
        resume,
    })
}

/// NetLogo's default number of BehaviorSpace threads: three quarters of the processors.
fn default_workers() -> usize {
    let n = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    (n * 3 / 4).max(1)
}

/// A folder name for an experiment: letters, digits, `-` and `_`; anything else becomes `_`.
fn folder_name(experiment: &str) -> String {
    let s: String = experiment
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() { "experiment".into() } else { s }
}

/// The values of the project's secrets, as environment variables for the bridge (Part K).
/// Fails if a required secret has no value.
fn secret_env(project: &Project) -> Result<Vec<(String, EnvValue)>, String> {
    let declared = project.secrets().map_err(|e| e.to_string())?;
    let names: Vec<String> = declared
        .required
        .iter()
        .chain(&declared.optional)
        .cloned()
        .collect();
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let store = crate::secrets::store()?;
    let found = logos_secrets::resolve(store.as_ref(), project.id().as_deref(), &names)
        .map_err(|e| e.to_string())?;
    if let Some(missing) = declared.required.iter().find(|n| !found.contains_key(*n)) {
        return Err(format!(
            "the required secret {missing} has no value; store it with `logos secrets set {missing}`"
        ));
    }
    Ok(found
        .into_iter()
        .map(|(name, (value, _))| (name, EnvValue(value.reveal().to_owned())))
        .collect())
}

/// A path as recorded: relative to the project folder with `/` separators when inside it,
/// otherwise absolute.
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

/// The engine and the files a run depends on (E3): the model, `logos.toml`, the experiments
/// file, and every project file the model reads or runs, with its data dictionary. Output
/// files are left out.
fn provenance(
    project: &Project,
    project_dir: &Path,
    model: &Path,
    setup_file: Option<&Path>,
    hello: &logos_bridge_client::protocol::HelloResult,
    netlogo_version: &str,
    secrets: Vec<String>,
) -> Result<Provenance, String> {
    let mut files = vec![HashedFile::new(
        &recorded_path(project_dir, model),
        "model",
        model,
    )];
    let toml = project_dir.join(PROJECT_FILE);
    if toml.is_file() {
        files.push(HashedFile::new(PROJECT_FILE, "project", &toml));
    }
    if let Some(s) = setup_file {
        files.push(HashedFile::new(
            &recorded_path(project_dir, s),
            "experiments",
            s,
        ));
    }
    for f in project.files().map_err(|e| e.to_string())? {
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

fn open_project(dir: &Path) -> Result<Project, String> {
    if dir.join(PROJECT_FILE).exists() {
        Project::open(dir).map_err(|e| e.to_string())
    } else {
        crate::check::detected_project(dir)
    }
}

fn absolute(p: &Path) -> Result<PathBuf, String> {
    std::path::absolute(p).map_err(|e| format!("{}: {e}", p.display()))
}

/// Set by the first Ctrl-C; a second one quits at once.
static CANCEL: AtomicBool = AtomicBool::new(false);

fn install_ctrl_c() {
    let _ = ctrlc::set_handler(|| {
        if CANCEL.swap(true, Ordering::SeqCst) {
            eprintln!("\nQuitting. Runs in progress are lost; finished runs are saved.");
            std::process::exit(130);
        }
        eprintln!(
            "\nStopping after the current step of each run in progress. Press Ctrl-C again \
             to quit at once."
        );
    });
}

pub fn run(args: &[&str]) -> Result<(), String> {
    let a = parse(args)?;
    // A new experiment: everything from the project and the arguments. A resumed one:
    // everything from the results folder.
    let (project_dir, model, experiment, setup_file, update_plots, resume) = match &a.resume {
        Some(dir) => {
            let r = ResultsDir::open(dir).map_err(|e| e.to_string())?;
            let m = r.meta.clone();
            (
                m.project,
                m.model,
                m.experiment,
                m.setup_file,
                m.update_plots,
                Some(r),
            )
        }
        None => {
            let project = open_project(&a.dir)?;
            let model = absolute(&project.model_path().map_err(|e| e.to_string())?)?;
            let setup = a.setup_file.as_deref().map(absolute).transpose()?;
            (
                absolute(&a.dir)?,
                model,
                a.experiment.clone().unwrap_or_default(),
                setup,
                a.update_plots,
                None,
            )
        }
    };
    let project = open_project(&project_dir)?;
    let version = project
        .engine()
        .and_then(|e| e.strip_prefix("netlogo-").map(str::to_owned))
        .unwrap_or_else(|| "7.0.4".into());
    let home = find_netlogo_home(&version).ok_or_else(|| {
        format!("NetLogo {version} not found; install it or set LOGOS_NETLOGO_HOME")
    })?;
    let jar = crate::check::bridge_jar().ok_or(
        "bridge jar not found; build it (cd bridge; .\\gradlew.bat jar) or set LOGOS_BRIDGE_JAR",
    )?;
    let mut config = BridgeConfig::new(home, &version, jar);
    config.env = secret_env(&project)?;
    let secret_names: Vec<String> = config.env.iter().map(|(n, _)| n.clone()).collect();

    let out_dir = match (&resume, &a.out) {
        (Some(r), _) => r.path().to_owned(),
        (None, Some(d)) => d.clone(),
        (None, None) => project_dir.join("results").join(format!(
            "{}-{}",
            folder_name(&experiment),
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        )),
    };
    if resume.is_none()
        && out_dir
            .join(logos_experiment::results::EXPERIMENT_FILE)
            .exists()
    {
        return Err(format!(
            "{} already holds results; resume them with --resume or choose another --out folder",
            out_dir.display()
        ));
    }
    let saved = match &resume {
        Some(r) => r.saved_runs().map_err(|e| e.to_string())?,
        None => Vec::new(),
    };

    install_ctrl_c();
    let bridge = Bridge::spawn(&config).map_err(|e| format!("bridge failed to start: {e}"))?;
    let netlogo_version = bridge
        .hello()
        .netlogo_version
        .clone()
        .unwrap_or_else(|| version.clone());
    let provenance = provenance(
        &project,
        &project_dir,
        &model,
        setup_file.as_deref(),
        bridge.hello(),
        &netlogo_version,
        secret_names,
    )?;
    if let Some(then) = resume.as_ref().and_then(|r| r.meta.provenance.as_ref()) {
        let changed = then.differences(&provenance);
        if !changed.is_empty() {
            let _ = bridge.shutdown();
            return Err(format!(
                "these results were started with different inputs, so new runs couldn't be \
                 compared with them: {}. Start a new run instead",
                changed.join("; ")
            ));
        }
    }
    let source = ExperimentSource {
        model: model.clone(),
        name: experiment.clone(),
        file: setup_file.clone(),
    };
    let options = RunOptions {
        workers: a.workers.unwrap_or_else(default_workers),
        update_plots,
        skip: saved.iter().map(|r| r.run_number).collect(),
        cancel: Some(&CANCEL),
        ..RunOptions::default()
    };

    // Set up when the experiment is loaded; any failure stops the experiment.
    struct Output {
        results: ResultsDir,
        table: logos_experiment::table::TableWriter<std::io::BufWriter<std::fs::File>>,
    }
    let output: Mutex<Option<Output>> = Mutex::new(None);
    let failure: Mutex<Option<String>> = Mutex::new(None);
    let fail = |e: String| {
        failure.lock().unwrap().get_or_insert(e);
        CANCEL.store(true, Ordering::SeqCst);
    };
    let total = Mutex::new(0usize);
    let done = Mutex::new(0usize);
    let resume_meta = resume.as_ref().map(|r| r.meta.clone());

    let outcome = run_experiment(
        &bridge,
        &source,
        &options,
        &|e, runs| {
            *total.lock().unwrap() = runs.len();
            let setup = || -> Result<Output, String> {
                let results = match &resume_meta {
                    Some(meta) => {
                        meta.check(e).map_err(|e| e.to_string())?;
                        ResultsDir::open(&out_dir).map_err(|e| e.to_string())?
                    }
                    None => ResultsDir::create(
                        &out_dir,
                        RunMeta {
                            experiment: e.name.clone(),
                            model: model.clone(),
                            project: project_dir.clone(),
                            setup_file: setup_file.clone(),
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
            if !saved.is_empty() {
                println!(
                    "Resuming {}: {} of {} runs already done",
                    e.name,
                    saved.len(),
                    e.runs.len()
                );
            }
            println!(
                "Running {}: {} runs on {} workspace(s). Results: {}",
                e.name,
                runs.len(),
                options.workers.min(runs.len()).max(1),
                out_dir.display()
            );
        },
        &|_, o| {
            let r = &o.result;
            if r.reason == logos_bridge_client::protocol::LabRunEnd::Halted {
                println!(
                    "  run {:>4}  stopped after {} steps; not saved",
                    r.run_number, r.steps
                );
                return;
            }
            if let Some(out) = output.lock().unwrap().as_mut() {
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
            let n = {
                let mut d = done.lock().unwrap();
                *d += 1;
                *d
            };
            println!(
                "  run {:>4}  {:>5}/{}  {:>7} steps  {:>8.1} s{}",
                r.run_number,
                n,
                total.lock().unwrap(),
                r.steps,
                r.elapsed_millis / 1000.0,
                if r.errors.is_empty() {
                    String::new()
                } else {
                    format!("  {} error(s)", r.errors.len())
                }
            );
            for e in &r.errors {
                println!(
                    "        {}: {}",
                    e.kind,
                    e.message.lines().next().unwrap_or("")
                );
            }
        },
    );
    let _ = bridge.shutdown();
    let outcome = outcome.map_err(describe_error)?;
    if let Some(e) = failure.into_inner().unwrap() {
        return Err(e);
    }
    for (when, e) in [
        ("pre-experiment", &outcome.pre_experiment_error),
        ("post-experiment", &outcome.post_experiment_error),
    ] {
        if let Some(e) = e {
            println!("Error in the {when} commands: {}: {}", e.kind, e.message);
        }
    }
    let Some(out) = output.into_inner().unwrap() else {
        return Err("the results folder was not set up".into());
    };
    drop(out.table);
    let saved_now = out.results.saved_runs().map_err(|e| e.to_string())?;
    let all = outcome.experiment.runs.len();
    if saved_now.len() < all {
        println!(
            "Stopped with {} of {all} runs done. To finish: logos run --resume \"{}\"",
            saved_now.len(),
            out_dir.display()
        );
        return Ok(());
    }
    let parquet = out
        .results
        .write_parquet(&outcome.experiment)
        .map_err(|e| e.to_string())?;
    let with_errors = saved_now.iter().filter(|r| !r.errors.is_empty()).count();
    println!(
        "{all} run(s) done in {:.1} s{}.\n  Table: {}\n  Parquet: {}",
        outcome.elapsed.as_secs_f64(),
        if with_errors > 0 {
            format!(", {with_errors} with runtime errors")
        } else {
            String::new()
        },
        out_dir
            .join(logos_experiment::results::TABLE_FILE)
            .display(),
        parquet.display()
    );
    Ok(())
}

/// Runner errors as the user should see them; an unknown experiment lists the ones there are.
fn describe_error(e: RunnerError) -> String {
    if let RunnerError::Client(ClientError::Rpc(rpc)) = &e
        && let Some(names) = rpc
            .data
            .as_ref()
            .and_then(|d| d.get("experiments"))
            .and_then(|n| n.as_array())
    {
        let names: Vec<&str> = names.iter().filter_map(|n| n.as_str()).collect();
        return if names.is_empty() {
            format!("{}; there are no experiments", rpc.message)
        } else {
            format!("{}; the experiments are: {}", rpc.message, names.join(", "))
        };
    }
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_arguments() {
        let a = parse(&[
            "N3-limit1",
            "models/aa4",
            "--workers",
            "4",
            "--update-plots",
        ])
        .unwrap();
        assert_eq!(a.experiment.as_deref(), Some("N3-limit1"));
        assert_eq!(a.dir, PathBuf::from("models/aa4"));
        assert_eq!(a.workers, Some(4));
        assert!(a.update_plots);
        assert!(parse(&[]).is_err());
        assert!(parse(&["e", "--workers", "0"]).is_err());
        assert!(parse(&["e", "--bogus"]).is_err());
        let r = parse(&["--resume", "results/x", "--workers", "2"]).unwrap();
        assert_eq!(r.resume, Some(PathBuf::from("results/x")));
        assert!(r.experiment.is_none());
        assert!(parse(&["--resume", "results/x", "--out", "y"]).is_err());
    }

    #[test]
    fn folder_names_are_safe() {
        assert_eq!(
            folder_name("Run metrics when count red > count white"),
            "Run_metrics_when_count_red___count_white"
        );
        assert_eq!(folder_name("N3-limit1"), "N3-limit1");
        assert_eq!(folder_name(""), "experiment");
    }
}
