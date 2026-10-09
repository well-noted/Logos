//! Running a BehaviorSpace experiment through the bridge.
//!
//! Each run follows BehaviorSpace's steps (`lab/Worker.scala` at NetLogo v7.0.4): set the run's
//! variables, run the setup commands, run the go commands until they `stop` or the time limit
//! is reached, then measure the metrics on a clone of the random number generator (D-14).
//! Several workspaces in one bridge run different runs at the same time (D-09).

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use logos_bridge_client::protocol::{NlValue, RpcError, RunEnd, RunStreams, WorkspaceId};
use logos_bridge_client::{Bridge, ClientError};

use crate::behaviorspace::{Experiment, RunSettings};

#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    #[error(transparent)]
    Client(#[from] ClientError),
    #[error("run {run_number}: {detail}")]
    Run { run_number: u64, detail: String },
    #[error("{0}")]
    Unsupported(String),
}

/// The result of one run.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub run_number: u64,
    /// Steps as BehaviorSpace counts them: completed go iterations, not counting a final one
    /// that executed `stop`.
    pub steps: u64,
    pub metrics: Vec<Result<NlValue, RpcError>>,
    pub elapsed: Duration,
    /// Which workspace (0-based worker index) ran it.
    pub worker: usize,
}

/// How long a single run may take before it is treated as hung.
const RUN_TIMEOUT: Duration = Duration::from_secs(3600);

/// Runs one run in an open workspace.
pub fn run_one(
    bridge: &Bridge,
    ws: WorkspaceId,
    experiment: &Experiment,
    settings: &RunSettings,
) -> Result<(u64, Vec<Result<NlValue, RpcError>>), RunnerError> {
    let set = settings.set_commands();
    if !set.is_empty() {
        bridge.command(ws, &set)?;
    }
    bridge.command(ws, &experiment.setup)?;
    let repeat = (experiment.time_limit > 0).then_some(experiment.time_limit);
    let run = bridge.run(ws, &experiment.go, repeat, &RunStreams::default())?;
    let done = bridge.wait_run_finished(ws, run, RUN_TIMEOUT)?;
    let steps = match done.reason {
        RunEnd::Stopped => done.iterations.saturating_sub(1),
        RunEnd::Completed => done.iterations,
        RunEnd::Halted | RunEnd::Error => {
            return Err(RunnerError::Run {
                run_number: settings.run_number,
                detail: format!("run ended with {:?}: {:?}", done.reason, done.error),
            });
        }
    };
    let metrics: Vec<&str> = experiment.metrics.iter().map(String::as_str).collect();
    let observed = bridge.observe(ws, &metrics)?;
    Ok((steps, observed.values))
}

/// Runs `runs` of `experiment` on `workers` workspaces of one bridge. `on_done` is called as
/// each run finishes, from the worker thread that ran it. Results are returned in run order.
pub fn run_experiment(
    bridge: &Bridge,
    model: &Path,
    experiment: &Experiment,
    runs: &[RunSettings],
    workers: usize,
    on_done: &(dyn Fn(&RunOutcome) + Sync),
) -> Result<Vec<RunOutcome>, RunnerError> {
    if experiment.run_metrics_every_step {
        return Err(RunnerError::Unsupported(
            "measuring metrics every step is not supported yet".into(),
        ));
    }
    if !experiment.exit_condition.trim().is_empty() {
        return Err(RunnerError::Unsupported(
            "stop conditions are not supported yet".into(),
        ));
    }
    let workers = workers.max(1).min(runs.len().max(1));
    let queue = Mutex::new(runs.iter());
    let results = Mutex::new(Vec::with_capacity(runs.len()));
    let first_error: Mutex<Option<RunnerError>> = Mutex::new(None);

    std::thread::scope(|scope| {
        for worker in 0..workers {
            let (queue, results, first_error) = (&queue, &results, &first_error);
            scope.spawn(move || {
                let ws = match bridge.open(model) {
                    Ok(r) => r.workspace,
                    Err(e) => {
                        first_error.lock().unwrap().get_or_insert(e.into());
                        return;
                    }
                };
                loop {
                    if first_error.lock().unwrap().is_some() {
                        break;
                    }
                    let Some(settings) = queue.lock().unwrap().next() else {
                        break;
                    };
                    let started = Instant::now();
                    match run_one(bridge, ws, experiment, settings) {
                        Ok((steps, metrics)) => {
                            let outcome = RunOutcome {
                                run_number: settings.run_number,
                                steps,
                                metrics,
                                elapsed: started.elapsed(),
                                worker,
                            };
                            on_done(&outcome);
                            results.lock().unwrap().push(outcome);
                        }
                        Err(e) => {
                            first_error.lock().unwrap().get_or_insert(e);
                            break;
                        }
                    }
                }
                let _ = bridge.dispose(ws);
            });
        }
    });

    if let Some(e) = first_error.into_inner().unwrap() {
        return Err(e);
    }
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|r| r.run_number);
    Ok(results)
}
