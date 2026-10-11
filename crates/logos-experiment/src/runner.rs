//! Running a BehaviorSpace experiment through the bridge.
//!
//! The bridge reads the experiment with NetLogo's own loader and runs each run with NetLogo's
//! own BehaviorSpace code (PROTOCOL.md section 5.4), so variable settings, stop conditions,
//! metric timing and error handling are BehaviorSpace's. This module schedules the runs: it
//! opens workspaces in one bridge (D-09), runs the pre-experiment commands in the first, hands
//! runs to workspaces as they become free, skips runs the caller already has (resume), stops
//! starting runs when asked, and runs the post-experiment commands at the end.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use logos_bridge_client::protocol::{LabError, LabRunResult, LoadedExperiment, WorkspaceId};
use logos_bridge_client::{Bridge, ClientError};

#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    #[error(transparent)]
    Client(#[from] ClientError),
    #[error("the model has compile errors: {0}")]
    Compile(String),
}

/// Where the experiment comes from.
#[derive(Debug, Clone)]
pub struct ExperimentSource {
    pub model: PathBuf,
    pub name: String,
    /// A separate BehaviorSpace XML file (`--setup-file` in headless NetLogo); `None` reads
    /// the experiment from the model.
    pub file: Option<PathBuf>,
}

/// How to run it.
pub struct RunOptions<'a> {
    /// Number of workspaces (parallel runs). At least 1.
    pub workers: usize,
    /// Update plots during runs. Off by default, as in headless BehaviorSpace.
    pub update_plots: bool,
    /// Runs not to run, because their results already exist.
    pub skip: BTreeSet<u64>,
    /// When set, no new runs are started, and runs in progress are stopped after their current
    /// step (they end as halted and are not measured at the end).
    pub cancel: Option<&'a AtomicBool>,
    /// How long one run may take before the bridge is treated as hung.
    pub run_timeout: Duration,
}

impl Default for RunOptions<'_> {
    fn default() -> Self {
        RunOptions {
            workers: 1,
            update_plots: false,
            skip: BTreeSet::new(),
            cancel: None,
            run_timeout: Duration::from_secs(7 * 24 * 3600),
        }
    }
}

/// A finished run and the workspace (0-based worker index) that ran it.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub result: LabRunResult,
    pub worker: usize,
}

/// What `run_experiment` did.
#[derive(Debug, Clone)]
pub struct ExperimentOutcome {
    pub experiment: LoadedExperiment,
    /// Runs that ran, in run-number order.
    pub runs: Vec<RunOutcome>,
    /// Runs neither run nor skipped, because of `cancel`. Runs stopped by `cancel` are in
    /// `runs`, with reason `Halted`.
    pub not_started: Vec<u64>,
    pub pre_experiment_error: Option<LabError>,
    pub post_experiment_error: Option<LabError>,
    pub elapsed: Duration,
}

/// Loads an experiment in a new workspace to see its runs, then disposes the workspace.
pub fn load_experiment(
    bridge: &Bridge,
    source: &ExperimentSource,
) -> Result<LoadedExperiment, RunnerError> {
    let ws = open(bridge, &source.model)?;
    let loaded = bridge.experiment_load(ws, &source.name, source.file.as_deref());
    let _ = bridge.dispose(ws);
    let e = loaded?;
    let _ = bridge.experiment_close(e.id);
    Ok(e)
}

fn open(bridge: &Bridge, model: &Path) -> Result<WorkspaceId, RunnerError> {
    let opened = bridge.open(model)?;
    if !opened.compiled {
        let messages: Vec<String> = opened.errors.iter().map(|e| e.message.clone()).collect();
        let _ = bridge.dispose(opened.workspace);
        return Err(RunnerError::Compile(messages.join("; ")));
    }
    Ok(opened.workspace)
}

/// Runs an experiment. `on_started` is called once the experiment is loaded, with the runs
/// that will run; `on_done` as each run ends, from the thread that ran it.
pub fn run_experiment(
    bridge: &Bridge,
    source: &ExperimentSource,
    options: &RunOptions,
    on_started: &dyn Fn(&LoadedExperiment, &[u64]),
    on_done: &(dyn Fn(&LoadedExperiment, &RunOutcome) + Sync),
) -> Result<ExperimentOutcome, RunnerError> {
    let started = Instant::now();
    let first = open(bridge, &source.model)?;
    let mut workspaces = vec![first];
    let result = run_in(
        bridge,
        source,
        options,
        &mut workspaces,
        on_started,
        on_done,
        started,
    );
    for ws in workspaces {
        let _ = bridge.dispose(ws);
    }
    result
}

fn run_in(
    bridge: &Bridge,
    source: &ExperimentSource,
    options: &RunOptions,
    workspaces: &mut Vec<WorkspaceId>,
    on_started: &dyn Fn(&LoadedExperiment, &[u64]),
    on_done: &(dyn Fn(&LoadedExperiment, &RunOutcome) + Sync),
    started: Instant,
) -> Result<ExperimentOutcome, RunnerError> {
    let first = workspaces[0];
    let experiment = bridge.experiment_load(first, &source.name, source.file.as_deref())?;
    let todo: Vec<u64> = (1..=experiment.runs.len() as u64)
        .filter(|r| !options.skip.contains(r))
        .collect();
    on_started(&experiment, &todo);

    // BehaviorSpace runs the pre-experiment commands in its first workspace before any run.
    let pre = bridge.experiment_command(first, experiment.id, "preExperiment")?;

    let workers = options.workers.max(1).min(todo.len().max(1));
    for _ in 1..workers {
        workspaces.push(open(bridge, &source.model)?);
    }

    let queue = Mutex::new(todo.iter().copied());
    let results = Mutex::new(Vec::with_capacity(todo.len()));
    let first_error: Mutex<Option<RunnerError>> = Mutex::new(None);
    let cancelled = || options.cancel.is_some_and(|c| c.load(Ordering::SeqCst));
    let experiment_ref = &experiment;

    let finished = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        // Stops the runs in progress when the caller cancels.
        let (finished_ref, all) = (&finished, workspaces.len());
        let ids: &[WorkspaceId] = workspaces;
        scope.spawn(move || {
            while finished_ref.load(Ordering::SeqCst) < all {
                if cancelled() {
                    for &ws in ids {
                        let _ = bridge.stop(ws);
                    }
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        for (worker, &ws) in workspaces.iter().enumerate() {
            let (queue, results, first_error) = (&queue, &results, &first_error);
            let finished = &finished;
            scope.spawn(move || {
                loop {
                    if first_error.lock().unwrap().is_some() || cancelled() {
                        break;
                    }
                    let Some(run_number) = queue.lock().unwrap().next() else {
                        break;
                    };
                    match bridge.experiment_run(
                        ws,
                        experiment_ref.id,
                        run_number,
                        options.update_plots,
                        options.run_timeout,
                    ) {
                        Ok(result) => {
                            let outcome = RunOutcome { result, worker };
                            on_done(experiment_ref, &outcome);
                            results.lock().unwrap().push(outcome);
                        }
                        Err(e) => {
                            first_error.lock().unwrap().get_or_insert(e.into());
                            break;
                        }
                    }
                }
                finished.fetch_add(1, Ordering::SeqCst);
            });
        }
    });

    if let Some(e) = first_error.into_inner().unwrap() {
        return Err(e);
    }
    let mut runs = results.into_inner().unwrap();
    runs.sort_by_key(|r| r.result.run_number);
    let ran: BTreeSet<u64> = runs.iter().map(|r| r.result.run_number).collect();
    let not_started: Vec<u64> = todo.into_iter().filter(|r| !ran.contains(r)).collect();

    // And the post-experiment commands after the last, unless the experiment was cut short.
    let halted = runs
        .iter()
        .any(|r| r.result.reason == logos_bridge_client::protocol::LabRunEnd::Halted);
    let post = if not_started.is_empty() && !halted {
        bridge
            .experiment_command(first, experiment.id, "postExperiment")?
            .error
    } else {
        None
    };
    let _ = bridge.experiment_close(experiment.id);
    Ok(ExperimentOutcome {
        experiment,
        runs,
        not_started,
        pre_experiment_error: pre.error,
        post_experiment_error: post,
        elapsed: started.elapsed(),
    })
}
