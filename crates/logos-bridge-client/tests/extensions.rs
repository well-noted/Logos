//! Phase 0 A7: models using the `gis` and `nw` extensions run in the bridge.
//!
//! Each model is opened from the NetLogo Models Library, a fixed sequence of its own example
//! procedures is run, and a set of values is read. The sequence runs in two workspaces with the
//! same seed, and the values must agree, which also checks that both extensions are
//! deterministic under a fixed seed. Ignored by default:
//!
//! ```text
//! cargo test -p logos-bridge-client --test extensions -- --ignored --test-threads=1 --nocapture
//! ```

use std::path::PathBuf;

use logos_bridge_client::protocol::{NlValue, ObserveResult, WorkspaceId};
use logos_bridge_client::{Bridge, BridgeConfig, find_netlogo_home};

const NETLOGO_VERSION: &str = "7.0.4";

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn spawn() -> (Bridge, PathBuf) {
    let home = find_netlogo_home(NETLOGO_VERSION).expect("NetLogo 7.0.4; set LOGOS_NETLOGO_HOME");
    let jar = repo().join("bridge/build/libs/logos-bridge-0.0.0.jar");
    let bridge =
        Bridge::spawn(&BridgeConfig::new(home.clone(), NETLOGO_VERSION, jar)).expect("bridge");
    (bridge, home)
}

/// Runs `commands` one at a time (so a failure names the procedure) and then observes.
fn exercise(
    bridge: &Bridge,
    ws: WorkspaceId,
    commands: &[&str],
    reporters: &[&str],
) -> ObserveResult {
    for c in commands {
        bridge
            .command(ws, c)
            .unwrap_or_else(|e| panic!("`{c}` failed: {e}"));
    }
    let r = bridge.observe(ws, reporters).unwrap();
    for (rep, v) in reporters.iter().zip(&r.values) {
        assert!(v.is_ok(), "`{rep}` failed: {v:?}");
    }
    r
}

fn check_model(relative: &str, commands: &[&str], reporters: &[&str]) -> Vec<NlValue> {
    let (bridge, home) = spawn();
    let model = home.join("models").join(relative);
    let a = bridge.open(&model).unwrap();
    assert!(a.compiled, "{relative} does not compile: {:?}", a.errors);
    let b = bridge.open(&model).unwrap();
    let first = exercise(&bridge, a.workspace, commands, reporters);
    let second = exercise(&bridge, b.workspace, commands, reporters);
    assert_eq!(
        first, second,
        "two workspaces with the same seed disagree on {relative}"
    );
    bridge.shutdown().unwrap();
    first.values.into_iter().map(Result::unwrap).collect()
}

#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn a7_gis_general_examples() {
    let reporters = [
        "count cities",
        "sum [population] of cities",
        "count cities with [ size = 3 ]",
        "count citizens",
        "count patches with [ pcolor = cyan ]",
        "sum [patch-population] of patches with [ patch-population > 0 ]",
        "count patches with [ (elevation <= 0) or (elevation >= 0) ]",
    ];
    let values = check_model(
        "Code Examples/Extensions Examples/gis/GIS General Examples.nlogox",
        &[
            "random-seed 42",
            "setup",
            "draw-countries",
            "draw-rivers",
            "draw-cities",
            "draw-large-cities",
            "draw-us-rivers-in-green",
            "make-city-turtles",
            "highlight-large-city-turtles",
            "draw/clear-city-turtle-labels",
            "draw/clear-country-labels",
            "create-citizens-in-countries",
            "display-rivers-in-patches",
            "display-population-in-patches",
            "display-elevation",
            "display-elevation-in-patches",
            "sample-elevation-with-patches",
            "display-gradient-in-patches",
        ],
        &reporters,
    );
    for (r, v) in reporters.iter().zip(&values) {
        eprintln!("A7 gis: {r} = {v:?}");
    }
    assert!(
        matches!(values[0], NlValue::Number(n) if n > 0.0),
        "cities were created"
    );
    assert!(
        matches!(values[3], NlValue::Number(n) if n > 0.0),
        "citizens were created"
    );
}

#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn a7_nw_general_examples() {
    let reporters = [
        "count turtles",
        "count links",
        "sort [ precision size 6 ] of turtles",
        "sort [ color ] of turtles",
        "length nw:louvain-communities",
        "[ precision nw:betweenness-centrality 6 ] of turtle 0",
    ];
    let values = check_model(
        "Code Examples/Extensions Examples/nw/NW General Examples.nlogox",
        &[
            "random-seed 42",
            "setup",
            "set links-to-use \"undirected\"",
            "preferential-attachment",
            "betweenness",
            "eigenvector",
            "closeness",
            "weak-component",
            "community-detection",
            "find-biggest-cliques",
            "small-world-ring",
            "lattice-2d",
            "generate-random",
            "layout-turtles",
            // nw: needs a context for the reporters below.
            "nw:set-context turtles undirected-edges",
        ],
        &reporters,
    );
    for (r, v) in reporters.iter().zip(&values) {
        let shown = format!("{v:?}");
        eprintln!("A7 nw: {r} = {}", &shown[..shown.len().min(100)]);
    }
    assert!(
        matches!(values[0], NlValue::Number(n) if n > 0.0),
        "nodes were generated"
    );
}
