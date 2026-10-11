//! Conformance checks that need NetLogo 7.0.4 and the bridge jar. Ignored by default:
//!
//! ```text
//! cargo test -p logos-experiment -- --ignored --test-threads=1 --nocapture
//! ```
//!
//! E1 (NetLogo's suite): the BehaviorSpace test cases in NetLogo's own repository
//! (`test/lab` at v7.0.4) give the same table output through Logos. The NetLogo source tree is
//! read from `LOGOS_NETLOGO_SRC` (default `corpus/.cache/NetLogo-7.0.4`); the test is skipped
//! when it isn't there.
//! A3/E1: AA4-Combined's `N3-limit1` gives the same table as NetLogo's headless BehaviorSpace
//! (reference table in corpus/reference).
//! A6: runs on parallel workspaces in one bridge give the same results as one at a time.
//!
//! `LOGOS_WORKERS` sets the number of parallel workspaces (default: number of cores, at most 8).

use std::path::{Path, PathBuf};

use logos_bridge_client::{Bridge, BridgeConfig, find_netlogo_home};
use logos_experiment::runner::{ExperimentSource, RunOptions, RunOutcome, run_experiment};
use logos_experiment::table::{Table, TableWriter};

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

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("logos-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A model with the given code and world, as `HeadlessWorkspace.initForTesting` makes one.
fn model(code: &str, (min_x, max_x, min_y, max_y): (i64, i64, i64, i64)) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<model version="NetLogo 7.0.4" snapToGrid="false">
  <code><![CDATA[{code}]]></code>
  <widgets>
    <view x="0" wrappingAllowedX="true" y="0" frameRate="30.0" minPycor="{min_y}" height="110" showTickCounter="true" patchSize="10.0" fontSize="10" wrappingAllowedY="true" width="110" tickCounterLabel="ticks" maxPycor="{max_y}" updateMode="1" maxPxcor="{max_x}" minPxcor="{min_x}"></view>
  </widgets>
  <info></info>
  <turtleShapes></turtleShapes>
  <linkShapes></linkShapes>
</model>
"#
    )
}

/// Runs an experiment and returns its table output.
fn table(bridge: &Bridge, source: &ExperimentSource, workers: usize) -> (String, Vec<RunOutcome>) {
    let rows = std::sync::Mutex::new(None::<TableWriter<Vec<u8>>>);
    let version = bridge
        .hello()
        .netlogo_version
        .clone()
        .unwrap_or_else(|| NETLOGO_VERSION.into());
    let model = source.model.to_string_lossy().into_owned();
    let options = RunOptions {
        workers,
        ..RunOptions::default()
    };
    let outcome = run_experiment(
        bridge,
        source,
        &options,
        &|e, _| {
            *rows.lock().unwrap() =
                Some(TableWriter::new(Vec::new(), &version, &model, e).unwrap());
        },
        &|_, o| {
            rows.lock()
                .unwrap()
                .as_mut()
                .unwrap()
                .write_run(&o.result)
                .unwrap()
        },
    )
    .unwrap();
    let text = String::from_utf8(rows.into_inner().unwrap().unwrap().into_inner()).unwrap();
    (text, outcome.runs)
}

/// Table output without the six metadata lines, with LF line ends, as NetLogo's tests compare.
fn body(text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    let lines: Vec<&str> = text.split('\n').skip(6).collect();
    lines.join("\n")
}

/// The test cases in NetLogo's `TestBehaviorSpace.scala` (netlogo-gui, v7.0.4) that check
/// table output in two dimensions: experiment, model code, world size (from -size to size).
const NETLOGO_CASES: &[(&str, &str, i64)] = &[
    ("testBehaviorSpace1", "globals [param1 param2 counter]", 0),
    ("testBehaviorSpace2", "globals [param1 param2 counter]", 0),
    ("testMultipleMetrics", "globals [counter param1]", 0),
    ("testNoMetrics1", "globals [counter param1]", 0),
    ("testNoMetrics2", "globals [counter param1]", 0),
    ("testImmediateExit", "globals [counter foo]", 0),
    ("testResizingWorld1", "", 0),
    ("testResizingWorld2", "", 0),
    ("testRandomSeed", "", 0),
    ("runNumber", "", 0),
    ("testStopping1", "globals [x]", 0),
    (
        "testStopping2",
        "globals [x] to go if x = 5 [ stop ] set x x + 1 end",
        0,
    ),
    (
        "badAtBeginning",
        "globals [ glob1 ] to setup set glob1 one-of patches end to go set glob1 5 end to-report bad-divide report glob1 / 5 end",
        0,
    ),
    ("metricsLocalRandomness", "globals [x]", 0),
    ("exitConditionLocalRandomness", "globals [x]", 0),
    ("metricGoBoom", "", 0),
    (
        "metricGoBoom2",
        "to setup clear-all create-turtles 1 reset-ticks end\nto go if not any? turtles [ stop ] if ticks = 10 [ ask turtles [ die ] ] tick end",
        0,
    ),
    ("setupCommandsGoBoom", "", 0),
    ("goCommandsGoBoom", "", 0),
    (
        "metricsWithSideEffects",
        "globals [g] to-report metric set g g + 1 report g end",
        0,
    ),
    (
        "stringMetrics",
        r#"to-report s1 report "x" end to-report s2 report "\"x\"" end"#,
        0,
    ),
    ("testBasicSubExperiment", "globals [a b]", 0),
    ("testMultipleSubExperiments", "globals [a b]", 0),
    ("testConstantOverride", "globals [a]", 0),
    ("testConstantReplace", "globals [a b]", 0),
    ("testMultipleConstantReplace", "globals [a b c]", 0),
    (
        "testSubExperimentRepetitionsNonSequential",
        "globals [a b]",
        0,
    ),
    ("testSubExperimentRepetitionsSequential", "globals [a b]", 0),
    ("testComplexSubExperiments", "globals [a b c]", 0),
    ("testSimpleLists", "globals [n]", 4),
    // Not included: testStats and testStatsExcludeMetrics. Their -table.csv files are inputs
    // to NetLogo's statistics test, which doesn't compare table output, and they don't match
    // what NetLogo 7.0.4's headless BehaviorSpace writes for those experiments (checked
    // 2026-10-11; Logos's output does match it).
];

fn netlogo_src() -> Option<PathBuf> {
    let dir = std::env::var_os("LOGOS_NETLOGO_SRC")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo().join("corpus/.cache/NetLogo-7.0.4"));
    dir.join("test/lab/protocols.xml").is_file().then_some(dir)
}

fn check_case(
    bridge: &Bridge,
    lab: &Path,
    source: &ExperimentSource,
    reference: &str,
    failures: &mut Vec<String>,
) {
    let expected = std::fs::read_to_string(lab.join(format!("{reference}-table.csv")))
        .unwrap()
        .replace("\r\n", "\n");
    let (text, _) = table(bridge, source, 1);
    let got = body(&text);
    if got == expected {
        eprintln!("  same    {reference}");
    } else {
        eprintln!("  DIFFERS {reference}\n--- NetLogo\n{expected}--- Logos\n{got}---");
        failures.push(reference.to_owned());
    }
}

#[test]
#[ignore = "needs NetLogo 7.0.4, the bridge jar and NetLogo's source tree"]
fn e1_netlogo_behaviorspace_suite() {
    let Some(src) = netlogo_src() else {
        eprintln!("NetLogo source tree not found; set LOGOS_NETLOGO_SRC. Skipped.");
        return;
    };
    let lab = src.join("test/lab");
    let dir = temp_dir("e1-suite");
    let bridge = spawn();
    let mut failures = Vec::new();
    for (name, code, size) in NETLOGO_CASES {
        let world = if *name == "testResizingWorld3" {
            (0, 1, 0, 1)
        } else {
            (-size, *size, -size, *size)
        };
        let path = dir.join(format!("{name}.nlogox"));
        std::fs::write(&path, model(code, world)).unwrap();
        let source = ExperimentSource {
            model: path,
            name: (*name).into(),
            file: Some(lab.join("protocols.xml")),
        };
        check_case(&bridge, &lab, &source, name, &mut failures);
    }
    // ResizingWorld3 starts from a 2 x 2 world.
    let path = dir.join("testResizingWorld3.nlogox");
    std::fs::write(&path, model("", (0, 1, 0, 1))).unwrap();
    let source = ExperimentSource {
        model: path,
        name: "testResizingWorld3".into(),
        file: Some(lab.join("protocols.xml")),
    };
    check_case(&bridge, &lab, &source, "testResizingWorld3", &mut failures);
    // Experiments stored in a model, as NetLogo's ModelWithIncludedExperiments test runs them.
    for n in 1..=5 {
        let source = ExperimentSource {
            model: lab.join("FireWithExperiments.nlogox"),
            name: format!("test{n}"),
            file: None,
        };
        check_case(
            &bridge,
            &lab,
            &source,
            &format!("FireWithExperiments{n}"),
            &mut failures,
        );
    }
    bridge.shutdown().unwrap();
    assert!(failures.is_empty(), "tables differ: {failures:?}");
}

/// Data rows sorted by run number, for comparing runs that finished in a different order. The
/// sort is stable, so the rows of one run keep their order.
fn sorted_rows(text: &str) -> Vec<Vec<String>> {
    let mut rows = Table::parse(text).unwrap().rows;
    rows.sort_by_key(|r| r[0].parse::<u64>().unwrap_or(0));
    rows
}

#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn a3_aa4_n3_limit1_matches_netlogo_behaviorspace() {
    let reference_path = repo().join("corpus/reference/aa4-n3-limit1-netlogo-7.0.4.csv");
    let reference = std::fs::read_to_string(&reference_path).unwrap();
    let expected = Table::parse(&reference).unwrap();
    assert_eq!(expected.experiment, "N3-limit1");

    let bridge = spawn();
    let workers = workers();
    let source = ExperimentSource {
        model: aa4(),
        name: "N3-limit1".into(),
        file: None,
    };
    let started = std::time::Instant::now();
    let (text, runs) = table(&bridge, &source, workers);
    let total = started.elapsed();
    let got = Table::parse(&text).unwrap();
    assert_eq!(got.columns, expected.columns);
    assert_eq!(runs.len(), 60, "30 seeds x 2 depletion rates");

    let (a, b) = (sorted_rows(&text), sorted_rows(&reference));
    let mismatches: Vec<String> = a
        .iter()
        .zip(&b)
        .filter(|(x, y)| x != y)
        .map(|(x, y)| format!("Logos {x:?}\nNetLogo {y:?}"))
        .collect();
    eprintln!(
        "A3: {} runs, {} rows, {} mismatches; {workers} workers, {:.1} s",
        runs.len(),
        a.len(),
        mismatches.len(),
        total.as_secs_f64()
    );
    for m in mismatches.iter().take(5) {
        eprintln!("  {m}");
    }
    assert_eq!(a.len(), b.len());
    assert!(mismatches.is_empty());
    bridge.shutdown().unwrap();
}

/// A6: AA4 with eight seeds on parallel workspaces matches the same runs on one workspace.
#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn a6_parallel_workspaces_match_one_at_a_time() {
    let dir = temp_dir("a6");
    let file = dir.join("a6.xml");
    std::fs::write(
        &file,
        r#"<experiments>
  <experiment name="a6" repetitions="1" sequentialRunOrder="true" runMetricsEveryStep="false" timeLimit="150">
    <setup>setup</setup>
    <go>go</go>
    <metrics>
      <metric>year</metric>
      <metric>count households</metric>
      <metric>sum [age] of households</metric>
      <metric>[ (list who xcor ycor) ] of max-one-of households [ who ]</metric>
    </metrics>
    <constants>
      <steppedValueSet variable="run-seed" first="1" step="1" last="8"></steppedValueSet>
    </constants>
  </experiment>
</experiments>
"#,
    )
    .unwrap();
    let source = ExperimentSource {
        model: aa4(),
        name: "a6".into(),
        file: Some(file),
    };
    let bridge = spawn();
    let started = std::time::Instant::now();
    let (parallel, parallel_runs) = table(&bridge, &source, 8);
    let parallel_time = started.elapsed();
    let started = std::time::Instant::now();
    let (sequential, _) = table(&bridge, &source, 1);
    let sequential_time = started.elapsed();

    assert_eq!(
        sorted_rows(&parallel),
        Table::parse(&sequential).unwrap().rows
    );
    let used: std::collections::BTreeSet<usize> = parallel_runs.iter().map(|r| r.worker).collect();
    eprintln!(
        "A6: 8 runs identical on 8 workspaces ({:.1} s, {} workers used) and 1 workspace ({:.1} s)",
        parallel_time.as_secs_f64(),
        used.len(),
        sequential_time.as_secs_f64()
    );
    bridge.shutdown().unwrap();
}
