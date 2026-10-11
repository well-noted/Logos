//! `logos run`: run a BehaviorSpace experiment (Phase 1, Part E).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use logos_bridge_client::{Bridge, BridgeConfig, ClientError, EnvValue, find_netlogo_home};
use logos_experiment::runner::{ExperimentSource, RunOptions, RunnerError, run_experiment};
use logos_experiment::table::TableWriter;
use logos_project::{PROJECT_FILE, Project};

pub const USAGE: &str =
    "  logos run EXPERIMENT [FOLDER] [--workers N] [--setup-file FILE] [--out DIR]
            [--update-plots]

  run            run a BehaviorSpace experiment of the model in FOLDER (default: the current
                 folder) and write NetLogo's table output
    --workers N      runs at the same time (default: three quarters of the processors, as in
                     NetLogo)
    --setup-file F   read the experiment from a BehaviorSpace XML file instead of the model
    --out DIR        where to write results (default: FOLDER/results/EXPERIMENT-DATE-TIME)
    --update-plots   update plots during runs (off by default, as in headless NetLogo)";

struct Args {
    experiment: String,
    dir: PathBuf,
    workers: Option<usize>,
    setup_file: Option<PathBuf>,
    out: Option<PathBuf>,
    update_plots: bool,
}

fn parse(args: &[&str]) -> Result<Args, String> {
    let mut experiment = None;
    let mut dir = None;
    let mut workers = None;
    let mut setup_file = None;
    let mut out = None;
    let mut update_plots = false;
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
            "--update-plots" => update_plots = true,
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            s if experiment.is_none() => experiment = Some(s.to_owned()),
            s if dir.is_none() => dir = Some(PathBuf::from(s)),
            s => return Err(format!("unexpected argument {s}")),
        }
    }
    Ok(Args {
        experiment: experiment.ok_or("name the experiment to run")?,
        dir: dir.unwrap_or_else(|| PathBuf::from(".")),
        workers,
        setup_file,
        out,
        update_plots,
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

fn open_project(dir: &Path) -> Result<Project, String> {
    if dir.join(PROJECT_FILE).exists() {
        Project::open(dir).map_err(|e| e.to_string())
    } else {
        crate::check::detected_project(dir)
    }
}

pub fn run(args: &[&str]) -> Result<(), String> {
    let a = parse(args)?;
    let project = open_project(&a.dir)?;
    let model = std::path::absolute(project.model_path().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
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

    let out_dir = match &a.out {
        Some(d) => d.clone(),
        None => a.dir.join("results").join(format!(
            "{}-{}",
            folder_name(&a.experiment),
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        )),
    };
    let table_path = out_dir.join("table.csv");
    if table_path.exists() {
        return Err(format!(
            "{} exists; choose another --out folder",
            table_path.display()
        ));
    }

    let bridge = Bridge::spawn(&config).map_err(|e| format!("bridge failed to start: {e}"))?;
    let netlogo_version = bridge
        .hello()
        .netlogo_version
        .clone()
        .unwrap_or_else(|| version.clone());
    let source = ExperimentSource {
        model: model.clone(),
        name: a.experiment.clone(),
        file: a.setup_file.clone(),
    };
    let options = RunOptions {
        workers: a.workers.unwrap_or_else(default_workers),
        update_plots: a.update_plots,
        ..RunOptions::default()
    };
    let table: Mutex<Option<TableWriter<std::io::BufWriter<std::fs::File>>>> = Mutex::new(None);
    let setup_error: Mutex<Option<String>> = Mutex::new(None);
    let total = Mutex::new(0usize);
    let done = Mutex::new(0usize);
    let model_text = model.to_string_lossy().into_owned();

    let outcome = run_experiment(
        &bridge,
        &source,
        &options,
        &|experiment, runs| {
            *total.lock().unwrap() = runs.len();
            let opened = std::fs::create_dir_all(&out_dir)
                .and_then(|_| std::fs::File::create_new(&table_path))
                .and_then(|f| {
                    TableWriter::new(
                        std::io::BufWriter::new(f),
                        &netlogo_version,
                        &model_text,
                        experiment,
                    )
                });
            match opened {
                Ok(w) => *table.lock().unwrap() = Some(w),
                Err(e) => {
                    *setup_error.lock().unwrap() =
                        Some(format!("cannot write {}: {e}", table_path.display()))
                }
            }
            println!(
                "Running {}: {} runs on {} workspace(s)",
                experiment.name,
                runs.len(),
                options.workers.min(runs.len()).max(1)
            );
        },
        &|_, o| {
            let r = &o.result;
            if let Some(w) = table.lock().unwrap().as_mut()
                && let Err(e) = w.write_run(r)
            {
                setup_error
                    .lock()
                    .unwrap()
                    .get_or_insert(format!("cannot write {}: {e}", table_path.display()));
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
    if let Some(e) = setup_error.into_inner().unwrap() {
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
    let with_errors = outcome
        .runs
        .iter()
        .filter(|r| !r.result.errors.is_empty())
        .count();
    println!(
        "{} run(s) in {:.1} s{}. Table: {}",
        outcome.runs.len(),
        outcome.elapsed.as_secs_f64(),
        if with_errors > 0 {
            format!(", {with_errors} with runtime errors")
        } else {
            String::new()
        },
        table_path.display()
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
        assert_eq!(a.experiment, "N3-limit1");
        assert_eq!(a.dir, PathBuf::from("models/aa4"));
        assert_eq!(a.workers, Some(4));
        assert!(a.update_plots);
        assert!(parse(&[]).is_err());
        assert!(parse(&["e", "--workers", "0"]).is_err());
        assert!(parse(&["e", "--bogus"]).is_err());
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
