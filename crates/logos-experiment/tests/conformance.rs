//! Phase 0 conformance checks that need NetLogo 7.0.4 and the bridge jar. Ignored by default:
//!
//! ```text
//! cargo test -p logos-experiment -- --ignored --test-threads=1 --nocapture
//! ```
//!
//! A3: AA4-Combined's `N3-limit1` experiment run through the bridge gives the same metrics as
//! NetLogo's own headless BehaviorSpace (reference table in corpus/reference).
//! A6: runs on parallel workspaces in one bridge give the same results as one at a time.
//!
//! `LOGOS_WORKERS` sets the number of parallel workspaces (default: number of cores, at most 8).

use std::path::PathBuf;
use std::time::Instant;

use logos_bridge_client::protocol::NlValue;
use logos_bridge_client::{Bridge, BridgeConfig, find_netlogo_home};
use logos_experiment::behaviorspace::{Experiment, Variable};
use logos_experiment::dump::{parse_dump, same_value};
use logos_experiment::find_experiment;
use logos_experiment::runner::{RunOutcome, run_experiment};
use logos_experiment::table::Table;

const NETLOGO_VERSION: &str = "7.0.4";

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn aa4() -> PathBuf {
    repo().join("corpus/own/aa4-combined/AA4-Combined.nlogox")
}

fn spawn() -> Bridge {
    let home = find_netlogo_home(NETLOGO_VERSION).expect("NetLogo 7.0.4; set LOGOS_NETLOGO_HOME");
    let jar = repo().join("bridge/build/libs/logos-bridge-0.0.0.jar");
    Bridge::spawn(&BridgeConfig::new(home, NETLOGO_VERSION, jar)).expect("bridge starts")
}

fn workers() -> usize {
    std::env::var("LOGOS_WORKERS")
        .ok()
        .and_then(|w| w.parse().ok())
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(2)
                .min(8)
        })
}

/// A variable value as the experiment file writes it (a NetLogo literal) and as the table
/// writes it (printed, strings without quotes) refer to the same value.
fn same_setting(literal: &str, table_cell: &str) -> bool {
    let unquoted = literal
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(literal);
    parse_dump(unquoted) == parse_dump(table_cell)
}

#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn a3_aa4_n3_limit1_matches_netlogo_behaviorspace() {
    let xml = std::fs::read_to_string(aa4()).unwrap();
    let experiment = find_experiment(&xml, "N3-limit1").unwrap();
    let runs = experiment.runs();
    assert_eq!(runs.len(), 60, "30 seeds x 2 depletion rates");

    let reference_path = repo().join("corpus/reference/aa4-n3-limit1-netlogo-7.0.4.csv");
    let table = Table::parse(&std::fs::read_to_string(&reference_path).unwrap()).unwrap();
    assert_eq!(table.experiment, "N3-limit1");
    assert_eq!(table.rows.len(), 60);

    // Our run numbering and settings must match BehaviorSpace's before any results are compared.
    for r in &runs {
        let row = table
            .final_row(r.run_number)
            .unwrap_or_else(|| panic!("reference has no run {}", r.run_number));
        for (name, literal) in &r.values {
            let col = table.column(name).unwrap();
            assert!(
                same_setting(literal, &row[col]),
                "run {}: {name} is {literal} here and {} in the reference",
                r.run_number,
                row[col]
            );
        }
    }

    let bridge = spawn();
    let workers = workers();
    let started = Instant::now();
    let outcomes = run_experiment(&bridge, &aa4(), &experiment, &runs, workers, &|o| {
        eprintln!(
            "run {:>2} done on worker {} in {:.1} s",
            o.run_number,
            o.worker,
            o.elapsed.as_secs_f64()
        )
    })
    .unwrap();
    let total = started.elapsed();

    let step_col = table.column("[step]").unwrap();
    let mut mismatches = Vec::new();
    let mut compared = 0;
    for o in &outcomes {
        let row = table.final_row(o.run_number).unwrap();
        if row[step_col] != o.steps.to_string() {
            mismatches.push(format!(
                "run {}: [step] {} here, {} in NetLogo",
                o.run_number, o.steps, row[step_col]
            ));
        }
        for (metric, value) in experiment.metrics.iter().zip(&o.metrics) {
            let cell = &row[table.column(metric).unwrap()];
            compared += 1;
            match value {
                Ok(v) if same_value(v, cell) => {}
                Ok(v) => mismatches.push(format!(
                    "run {}: {metric} = {} here, {} in NetLogo",
                    o.run_number,
                    short(&format!("{v:?}")),
                    short(cell)
                )),
                Err(e) => mismatches.push(format!("run {}: {metric} failed: {e}", o.run_number)),
            }
        }
    }
    eprintln!(
        "A3: {} runs, {compared} metric values compared, {} mismatches; {workers} workers, {:.1} s",
        outcomes.len(),
        mismatches.len(),
        total.as_secs_f64()
    );
    for m in mismatches.iter().take(20) {
        eprintln!("  {m}");
    }
    assert!(mismatches.is_empty(), "{} mismatches", mismatches.len());
    bridge.shutdown().unwrap();
}

/// A6: AA4 with eight seeds on parallel workspaces matches the same runs on one workspace.
#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn a6_parallel_workspaces_match_one_at_a_time() {
    let experiment = Experiment {
        name: "a6".into(),
        repetitions: 1,
        sequential_run_order: true,
        run_metrics_every_step: false,
        setup: "setup".into(),
        go: "go".into(),
        time_limit: 150,
        exit_condition: String::new(),
        metrics: vec![
            "year".into(),
            "count households".into(),
            "sum [age] of households".into(),
            "[ (list who xcor ycor) ] of max-one-of households [ who ]".into(),
        ],
        variables: vec![Variable {
            name: "run-seed".into(),
            values: (1..=8).map(|s| s.to_string()).collect(),
        }],
    };
    let runs = experiment.runs();
    let bridge = spawn();

    let started = Instant::now();
    let parallel = run_experiment(&bridge, &aa4(), &experiment, &runs, 8, &|_| {}).unwrap();
    let parallel_time = started.elapsed();
    let started = Instant::now();
    let sequential = run_experiment(&bridge, &aa4(), &experiment, &runs, 1, &|_| {}).unwrap();
    let sequential_time = started.elapsed();

    let values = |o: &[RunOutcome]| -> Vec<(u64, u64, Vec<NlValue>)> {
        o.iter()
            .map(|r| {
                (
                    r.run_number,
                    r.steps,
                    r.metrics.iter().map(|m| m.clone().unwrap()).collect(),
                )
            })
            .collect()
    };
    assert_eq!(values(&parallel), values(&sequential));
    let used: std::collections::BTreeSet<usize> = parallel.iter().map(|r| r.worker).collect();
    eprintln!(
        "A6: 8 runs identical on 8 workspaces ({:.1} s, {} workers used) and 1 workspace ({:.1} s)",
        parallel_time.as_secs_f64(),
        used.len(),
        sequential_time.as_secs_f64()
    );
    bridge.shutdown().unwrap();
}

fn short(s: &str) -> String {
    if s.len() > 120 {
        format!("{}...", &s[..120])
    } else {
        s.to_owned()
    }
}
