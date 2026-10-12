# Phase 2 results

Measurements against the criteria in `docs/phase-2-spec.md`, part by part, as the steps land.

## Part F: framework spike

The spike is in `spikes/f1-tauri` (Tauri 2.12, plain JavaScript front end). It streams AA4's view frames and the A5 model's agent state into a Tauri window through a Tauri channel, `ipc::Response` polling, and a WebSocket on 127.0.0.1, and checks F1 to F3 itself.

Measured 2026-10-12 on Thomas's machine (Surface Laptop Studio, WebView2, a 120 Hz display at 150% scaling, release build): three runs with NetLogo and one with `--synthetic`, back to back.

| # | Criterion | Result | Status |
|---|---|---|---|
| F1 | AA4 view frames in a Tauri window, Windows | Every transport drew at least 99% of the frames the bridge sent, in every run (channel 100%). Frames drawn per second followed AA4's iteration rate, because below 30 iterations per second the bridge sends a frame every iteration: run 1, channel 28.7 fps at 42 iterations/s (A4: 28.7); later tests 18.7 to 26.7 fps while AA4 slowed to 21 to 36 iterations/s. With synthetic frames, all three transports drew 30.0 fps, 324 of 324 | Transport met; AA4 rate below target after the first test (see below) |
| F2 | Agent state for 10,000 turtles, Windows | 29.5 to 30.1 updates/s through every transport in every run; synthetic 30.0 | Met |
| F3 | Room for the rest of the app | Main-thread time per drawn frame at most 0.3 ms (view) and 1.1 ms (agents) at the 95th percentile; no late animation frames in view tests, at most 0.1% in agents tests | Met |
| F4 | Linux and macOS | Not yet measured | Pending |

Median latency from the Rust side to the front end's handler: WebSocket 1.8 to 3.4 ms, channel 9 to 23 ms, polling 12 to 22 ms.

What limited F1 was AA4's iteration rate, which fell from run to run (42, then 32, 24 iterations per second for the first test of each run) while the transports delivered every frame. A likely cause is the laptop slowing down under several minutes of full load (thermal limits); the agents model, run in the same tests, did not slow down in the same way because its frame rate is capped well below its iteration rate. This is a reading of the numbers, not yet a measured cause. A run of the Phase 0 viewer and the spike one after the other, with the laptop cool, would separate the two.

No bridge process was left running: the one Java process listed afterwards was started during `gradlew jar`, which leaves a Gradle daemon.

## App steps so far

Built in the cloud and checked there against `app/dev/mock-bridge.py` (Linux, WebKitGTK under Xvfb), not yet against NetLogo. The walkthrough in the Project handoff `docs/handoffs/test-runs-app1.md` checks them on Windows with NetLogo.

| Branch | What it adds | Cloud checks |
|---|---|---|
| `phase2/widgets` | Typed widget model in `logos-nlogox` | Unit tests |
| `phase2/app-shell` | Tauri host, launcher, model screen (Interface, Info, Code), running, view frames, monitors, inspector, themes | Unit tests; screenshots of each screen |
| `phase2/app-features` | Language server in the Code tab, Outline and Files panels, projects and secrets, experiments, dashboard and results | Unit tests; diagnostics and completion, an experiment run to `table.csv`, no secret value in any file or log |
| `phase2/interface-editing` | Edit mode: move and resize widgets, add and delete widgets, property sheet, undo and redo | 18 `logos-nlogox` widget tests; dragging, adding a slider and editing its properties in screenshots |

Workarounds until protocol 1: the snapshot is a zero-tick run (G15); writes during a run stop and restart it (G1); speeds below full are stepped runs with a delay (G2); Apply saves and reopens the model, which clears the world (G5); plots show no data (G4).

## Spike check in the cloud

Check of the spike itself, 2026-10-11, in the cloud (Linux, WebKitGTK 2.52 under Xvfb with software rendering, `--synthetic`, so no NetLogo and no GPU). These numbers check that the spike works; they are not criterion measurements. Every transport delivered all 324 view frames and about 30 agent updates per second with no gaps. Median latency from Rust to the handler was about 5 ms for the channel and polling and 1.5 ms for the WebSocket. Main-thread time per drawn frame was at most 2 ms at the 95th percentile, except when all 10,000 turtles were also drawn as rectangles on the canvas (15 to 17 ms median). WebKit rounds `performance.now()` to 1 ms, so times below that read as 0.
