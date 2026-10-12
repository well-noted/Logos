//! Tests that start the real bridge on an installed NetLogo 7.0.4.
//!
//! They are ignored by default because they need NetLogo and the bridge jar. Run them with:
//!
//! ```text
//! cd bridge && .\gradlew.bat jar && cd ..
//! cargo test -p logos-bridge-client -- --ignored --test-threads=1
//! ```
//!
//! Phase 0 criteria covered here: A1 (start with NetLogo's bundled runtime from a path with
//! spaces), A2 (time to first result, printed), A10 (bridge exits when its stdin closes),
//! A11 (errors come back structured and the bridge keeps running).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use logos_bridge_client::protocol::{NlValue, codes};
use logos_bridge_client::{Bridge, BridgeConfig, ClientError, find_netlogo_home};

const NETLOGO_VERSION: &str = "7.0.4";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn aa4() -> PathBuf {
    repo_root().join("corpus/own/aa4-combined/AA4-Combined.nlogox")
}

fn config() -> BridgeConfig {
    let home = find_netlogo_home(NETLOGO_VERSION)
        .unwrap_or_else(|| panic!("NetLogo {NETLOGO_VERSION} not found; set LOGOS_NETLOGO_HOME"));
    let jar = repo_root().join("bridge/build/libs/logos-bridge-0.0.0.jar");
    BridgeConfig::new(home, NETLOGO_VERSION, jar)
}

fn spawn() -> Bridge {
    Bridge::spawn(&config()).unwrap_or_else(|e| panic!("bridge failed to start: {e}"))
}

fn number(v: &Result<NlValue, logos_bridge_client::protocol::RpcError>) -> f64 {
    match v {
        Ok(NlValue::Number(n)) => *n,
        other => panic!("expected a number, got {other:?}"),
    }
}

/// A1: the bridge runs on NetLogo's bundled Java runtime from the install folder.
#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn a1_starts_on_the_bundled_runtime() {
    let config = config();
    let bridge = spawn();
    let hello = bridge.hello();
    assert_eq!(hello.netlogo_version.as_deref(), Some(NETLOGO_VERSION));
    if config.java.is_none() {
        // NetLogo 7.0.4 bundles Java 17.
        assert!(
            hello.java_version.starts_with("17."),
            "java {}",
            hello.java_version
        );
    }
    eprintln!(
        "A1: NetLogo {:?} on Java {} ({}) from {}",
        hello.netlogo_version,
        hello.java_version,
        hello.java_vendor,
        config.java_executable().display()
    );
    bridge.shutdown().unwrap();
}

/// A2: time from starting the bridge to the first reporter result on an opened model.
#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn a2_time_to_first_result() {
    let start = Instant::now();
    let bridge = spawn();
    let after_hello = start.elapsed();
    let ws = bridge.open(aa4()).unwrap().workspace;
    let after_open = start.elapsed();
    let r = bridge.observe(ws, &["count patches"]).unwrap();
    let total = start.elapsed();
    assert_eq!(number(&r.values[0]), 80.0 * 120.0);
    eprintln!(
        "A2: hello {:?}, open AA4 {:?}, first result {:?} (target < 5 s)",
        after_hello, after_open, total
    );
    assert!(
        total < Duration::from_secs(30),
        "far outside the 5 s target: {total:?}"
    );
    bridge.shutdown().unwrap();
}

/// A10: when the client goes away its end of the stdin pipe closes, and the bridge must exit.
#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn a10_bridge_exits_when_stdin_closes() {
    let bridge = spawn();
    let ws = bridge.open(aa4()).unwrap().workspace;
    bridge.command(ws, "set run-seed 1 setup").unwrap();
    let start = Instant::now();
    let status = bridge.close_stdin_and_wait(Duration::from_secs(5));
    let took = start.elapsed();
    let status = status.unwrap_or_else(|| panic!("bridge still running after {took:?}"));
    eprintln!("A10: exited with {status} after {took:?}");
    assert!(status.success());
    assert!(
        took < Duration::from_secs(2),
        "took {took:?}, A10 allows 2 s"
    );
}

#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn shutdown_exits_cleanly() {
    let bridge = spawn();
    let status = bridge.shutdown().unwrap();
    assert!(status.success());
}

/// A11: errors come back structured and the workspace stays usable.
#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn a11_errors_are_structured() {
    let bridge = spawn();
    let ws = bridge.open(aa4()).unwrap().workspace;

    match bridge.command(ws, "crt 1 fd") {
        Err(ClientError::Rpc(e)) => {
            assert_eq!(e.code, codes::COMPILER_ERROR);
            let span = &e.data.as_ref().unwrap()["span"];
            assert_eq!(span["start"], 6);
            assert_eq!(span["end"], 8);
        }
        other => panic!("expected a compiler error, got {other:?}"),
    }
    match bridge.command(ws, "ask turtle 999999 [ fd 1 ]") {
        Err(ClientError::Rpc(e)) => assert_eq!(e.code, codes::RUNTIME_ERROR),
        other => panic!("expected a runtime error, got {other:?}"),
    }
    bridge.command(ws, "set run-seed 1 setup").unwrap();
    let r = bridge.observe(ws, &["year"]).unwrap();
    assert!(number(&r.values[0]) > 0.0);
    bridge.shutdown().unwrap();
}

/// The same seed gives the same AA4 run in two separate bridge processes.
#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn same_seed_same_run_across_processes() {
    let reporters = [
        "year",
        "count households",
        "sum [age] of households",
        "[ (list who xcor ycor) ] of max-one-of households [ who ]",
    ];
    let run = || {
        let bridge = spawn();
        let ws = bridge.open(aa4()).unwrap().workspace;
        bridge.command(ws, "set run-seed 3 setup").unwrap();
        bridge.command(ws, "repeat 60 [ go ]").unwrap();
        let r = bridge.observe(ws, &reporters).unwrap();
        bridge.shutdown().unwrap();
        r
    };
    let first = run();
    let second = run();
    assert_eq!(first, second);
    eprintln!("AA4 seed 3 after 60 steps: {:?}", first.values);
}

/// workspace.run from Rust: AA4 runs until it stops itself, with view frames streamed.
#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn run_streams_view_frames_until_the_model_stops() {
    use logos_bridge_client::protocol::{DataFrame, RunEnd, RunStreams, StreamRate};
    let bridge = spawn();
    let ws = bridge.open(aa4()).unwrap().workspace;
    bridge.command(ws, "set run-seed 1 setup").unwrap();
    let streams = RunStreams {
        view: Some(StreamRate {
            every_ticks: 1,
            max_fps: 1000.0,
            update: Default::default(),
        }),
        agents: None,
        observe: None,
    };
    let run = bridge.run(ws, "go", None, &streams).unwrap();
    let done = bridge
        .wait_run_finished(ws, run, Duration::from_secs(120))
        .unwrap();
    assert_eq!(done.reason, RunEnd::Stopped);
    let mut frames = Vec::new();
    while let Some(DataFrame::View(v)) = bridge.next_data_frame(Duration::from_secs(1)) {
        frames.push(v);
    }
    assert!(frames.len() >= 2);
    for (i, f) in frames.iter().enumerate() {
        assert_eq!(f.sequence, i as u64);
        assert_eq!((f.width, f.height), (320, 480));
    }
    assert_eq!(frames.last().unwrap().ticks, done.ticks);
    eprintln!(
        "run: {} iterations in {:.0} ms, {} frames",
        done.iterations,
        done.elapsed_millis,
        frames.len()
    );
    bridge.shutdown().unwrap();
}

/// Part of A4: AA4 run speed with and without view frames, printed for docs/phase-0-results.md.
#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn a4_run_speed_with_and_without_frames() {
    use logos_bridge_client::protocol::{RunStreams, StreamRate};
    let bridge = spawn();
    let measure = |streams: &RunStreams| {
        let ws = bridge.open(aa4()).unwrap().workspace;
        bridge.command(ws, "set run-seed 1 setup").unwrap();
        let run = bridge.run(ws, "go", None, streams).unwrap();
        let done = bridge
            .wait_run_finished(ws, run, Duration::from_secs(300))
            .unwrap();
        let mut frames = 0;
        while bridge.try_data_frame().is_some() {
            frames += 1;
        }
        bridge.dispose(ws).unwrap();
        (done.iterations, done.elapsed_millis, frames)
    };
    let (it0, ms0, _) = measure(&RunStreams::default());
    let (it1, ms1, f1) = measure(&RunStreams {
        view: Some(StreamRate {
            every_ticks: 1,
            max_fps: 30.0,
            update: Default::default(),
        }),
        agents: None,
        observe: None,
    });
    let (it2, ms2, f2) = measure(&RunStreams {
        view: Some(StreamRate {
            every_ticks: 1,
            max_fps: 1000.0,
            update: Default::default(),
        }),
        agents: None,
        observe: None,
    });
    assert_eq!(it0, it1);
    assert_eq!(it0, it2);
    eprintln!(
        "A4: no frames {it0} iterations in {ms0:.0} ms ({:.1}/s); \
         30 fps cap {ms1:.0} ms, {f1} frames ({:.1} fps); \
         every iteration {ms2:.0} ms, {f2} frames ({:.1} fps)",
        it0 as f64 * 1000.0 / ms0,
        f1 as f64 * 1000.0 / ms1,
        f2 as f64 * 1000.0 / ms2
    );
    bridge.shutdown().unwrap();
}
