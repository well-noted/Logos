//! BehaviorSpace experiments for Logos: run them through the bridge and write NetLogo's
//! output formats.

pub mod behaviorspace;
pub mod dump;
pub mod parquet;
pub mod results;
pub mod runner;
pub mod table;

pub use behaviorspace::{
    Experiment, ExperimentError, RunSettings, find_experiment, parse_experiments,
};
pub use runner::{
    ExperimentOutcome, ExperimentSource, RunOptions, RunOutcome, RunnerError, load_experiment,
    run_experiment,
};
