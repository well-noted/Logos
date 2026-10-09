//! Phase 0 A5: agent-state streaming with 10,000 turtles. Ignored by default:
//!
//! ```text
//! cargo test -p logos-bridge-client --test agents -- --ignored --test-threads=1 --nocapture
//! ```
//!
//! The model is generated here: 10,000 turtles that turn and move every tick, and 50 patches
//! that change color every tick, in a 101 x 101 world. Each setting runs `go` for a fixed
//! time and counts the agent frames that arrive and decode.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use logos_bridge_client::protocol::{
    AgentFrame, DataFrame, NlValue, RunEnd, RunStreams, StreamRate, WorkspaceId,
};
use logos_bridge_client::{Bridge, BridgeConfig, find_netlogo_home};

const NETLOGO_VERSION: &str = "7.0.4";
const TURTLES: usize = 10_000;
const RUN_FOR: Duration = Duration::from_secs(5);

const CODE: &str = "
to setup
  clear-all
  create-turtles 10000 [ setxy random-xcor random-ycor set color one-of base-colors ]
  reset-ticks
end
to go
  ask turtles [ rt random 30 - 15 fd 0.2 ]
  ask n-of 50 patches [ set pcolor one-of base-colors ]
  tick
end
";

fn model() -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<model version="NetLogo 7.0.4" snapToGrid="false">
  <code><![CDATA[{CODE}]]></code>
  <widgets>
    <view x="0" wrappingAllowedX="true" y="0" frameRate="30.0" minPycor="-50" height="414" showTickCounter="true" patchSize="4.0" fontSize="10" wrappingAllowedY="true" width="414" tickCounterLabel="ticks" maxPycor="50" updateMode="1" maxPxcor="50" minPxcor="-50"></view>
  </widgets>
  <info></info>
  <turtleShapes></turtleShapes>
  <linkShapes></linkShapes>
</model>
"#
    )
}

fn spawn() -> Bridge {
    let home = find_netlogo_home(NETLOGO_VERSION).expect("NetLogo 7.0.4; set LOGOS_NETLOGO_HOME");
    let jar = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join("bridge/build/libs/logos-bridge-0.0.0.jar");
    Bridge::spawn(&BridgeConfig::new(home, NETLOGO_VERSION, jar)).expect("bridge")
}

struct Measured {
    iterations: u64,
    seconds: f64,
    frames: usize,
    frame_bytes: usize,
    last: Option<AgentFrame>,
}

/// Runs `go` for `RUN_FOR`, draining agent frames as they arrive, then stops the run.
fn measure(bridge: &Bridge, ws: WorkspaceId, streams: &RunStreams) -> Measured {
    bridge.command(ws, "random-seed 1 setup").unwrap();
    let run = bridge.run(ws, "go", None, streams).unwrap();
    let mut frames = 0;
    let mut last = None;
    let mut take = |f: Option<DataFrame>| {
        if let Some(DataFrame::Agents(a)) = f {
            assert_eq!(a.turtles.len(), TURTLES);
            frames += 1;
            last = Some(*a);
        }
    };
    let deadline = Instant::now() + RUN_FOR;
    while Instant::now() < deadline {
        take(bridge.next_data_frame(Duration::from_millis(20)));
    }
    bridge.stop(ws).unwrap();
    let done = bridge
        .wait_run_finished(ws, run, Duration::from_secs(30))
        .unwrap();
    assert_eq!(done.reason, RunEnd::Halted);
    while let Some(f) = bridge.next_data_frame(Duration::from_millis(200)) {
        take(Some(f));
    }
    let frame_bytes = last.as_ref().map_or(0, |a: &AgentFrame| {
        25 + 20 + a.patch_rgba.len() + 4 + a.turtles.len() * (8 * 5 + 4 + 2 + 2 + 1)
    });
    Measured {
        iterations: done.iterations,
        seconds: done.elapsed_millis / 1000.0,
        frames,
        frame_bytes,
        last,
    }
}

#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn a5_agent_state_with_10000_turtles() {
    let bridge = spawn();
    let ws = bridge.open_string(&model()).unwrap();
    assert!(ws.compiled, "{:?}", ws.errors);
    let ws = ws.workspace;
    let agents = |max_fps: f64| RunStreams {
        agents: Some(StreamRate {
            every_ticks: 1,
            max_fps,
        }),
        ..RunStreams::default()
    };

    let none = measure(&bridge, ws, &RunStreams::default());
    let capped = measure(&bridge, ws, &agents(30.0));
    let every = measure(&bridge, ws, &agents(1e9));

    // The last frame shows the state the run ended in.
    let last = every.last.as_ref().expect("frames arrived");
    let obs = bridge
        .observe(
            ws,
            &[
                "ticks",
                "[ xcor ] of turtle 4321",
                "[ heading ] of turtle 4321",
            ],
        )
        .unwrap();
    let num = |i: usize| match &obs.values[i] {
        Ok(NlValue::Number(n)) => *n,
        other => panic!("{other:?}"),
    };
    assert_eq!(last.ticks, Some(num(0)));
    assert_eq!(last.turtles.xcor[4321], num(1));
    assert_eq!(last.turtles.heading[4321], num(2));
    assert_eq!(last.patch_rgba.len(), 101 * 101 * 4);

    let rate = |m: &Measured| m.iterations as f64 / m.seconds;
    let fps = |m: &Measured| m.frames as f64 / m.seconds;
    eprintln!(
        "A5: {TURTLES} turtles, frame {:.0} KB; no frames {:.1} ticks/s; \
         30 fps cap {:.1} frames/s ({:.1} ticks/s); every tick {:.1} frames/s ({:.1} ticks/s)",
        every.frame_bytes as f64 / 1024.0,
        rate(&none),
        fps(&capped),
        rate(&capped),
        fps(&every),
        rate(&every),
    );
    // Per-frame cost on the bridge side, from the slowdown when every tick sends a frame.
    if every.frames > 0 {
        let per_tick_none = 1.0 / rate(&none);
        let per_tick_every = 1.0 / rate(&every);
        eprintln!(
            "A5: about {:.1} ms per frame (read, encode, send, decode)",
            (per_tick_every - per_tick_none) * 1000.0
        );
    }
    assert!(
        fps(&capped) >= 29.0,
        "A5 target is 30 updates/s, got {:.1}",
        fps(&capped)
    );
    bridge.shutdown().unwrap();
}
