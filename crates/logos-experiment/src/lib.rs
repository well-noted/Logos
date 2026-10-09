//! BehaviorSpace experiments for Logos: read them from `.nlogox`, run them through the bridge,
//! and compare the results with NetLogo's own table output.

pub mod behaviorspace;
pub mod dump;
pub mod runner;
pub mod table;

pub use behaviorspace::{
    Experiment, ExperimentError, RunSettings, find_experiment, parse_experiments,
};
