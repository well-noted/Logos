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
| `phase2/world-settings` | World size, patch size, wrapping and the other view settings in the view's property sheet; the view's size follows the world | A `logos-nlogox` test that setting the world back gives the original bytes; patch size 4 to 3 in screenshots |
| `phase2/search` | Search and replace across code, Info and widgets (whole words by NetLogo's name rules, case, regular expressions, a preview of each replacement); git branch and changed files in the status bar | 5 front-end tests; replacing `fertility` with `birth-rate` in AA4 changed only the 9 matches (byte diff) |
| `phase2/recovery` | Autosave every 30 s and on focus loss to the app data folder, restore with a diff after a crash, reload or ask when another program changes the model | Diff and autosave tests; killing the app and reopening offered the edits with a diff; a change on disk was reloaded within 2 s, and with unsaved edits the app asked |
| `phase2/experiment-editor` | The experiment document edits BehaviorSpace settings in the model: name, value sets (enumerated and stepped, add and remove), repetitions, run order, metrics, setup and go commands, stop condition and time limit where the experiment has them; duplicate and delete; shared undo with interface edits | 5 `logos-nlogox` tests (edits undone by hand give the original bytes, CRLF files stay CRLF); 3 front-end tests; adding a value to `crowding-limit` in AA4 changed one line, and a duplicate is a byte copy of its source |
| `phase2/view-access` | "Describe the view" (Ctrl+Shift+D, or D on the focused view): ticks, world size and counts of turtles, links and each breed, read out by screen readers; arrow keys choose a patch on the focused view and Enter inspects it | Screenshots against the mock bridge; NVDA not yet tried |
| `phase2/data-files` | Input and output files open from the Files activity as a table preview (one row per line, or a stream of values grouped by the dictionary's columns, or text), with a form for the file's data dictionary; saving writes `<file>.dictionary.toml` beside it (unknown keys kept) and names it in `logos.toml` | `logos-project` writer test; 3 `logos-app` tests; AA4's `water.txt` (648 values on one line) shown as 108 rows once a 6-column dictionary was saved |
| `phase2/project-form` | The Project document is a form over `logos.toml`: engine, file roles and descriptions, adding files, required and optional secret names, Python requirements and interpreter, with "View source"; other keys and comments in the file are kept. The Python check no longer opens a console window on Windows | A `logos-app` test (comments and unknown keys kept, invalid secret names refused); adding a secret and a description in screenshots |
| `phase2/bridge-output` | The bridge sends what models print (`print`, `show`, `output-print`) as `workspace.output`, with `outputArea` telling Command Center text from Output widget text. Before, it went to the bridge's log, so `show count households` printed nothing in the app | `OutputTest` (needs NetLogo; not run in the cloud) |
| `phase2/walkthrough-fixes` | Fixes from Thomas's first walkthrough (2026-10-12): interface right-click menus (add here, edit, delete), Add places the widget where you click, Shift-drag snapping with guides; grouped and filterable outline; Command Center colors and its own log; one undo history across code, Info, interface and experiments (I3), so undoing a replace restores code and Info too; visible selection and whole-name matches in the code editor, Ctrl+click to definition, themed rename prompt; Apply's "Don't ask again" with a Settings switch; setting secret values in place; installed fonts in Settings; Go to commands leave other documents; tabs side by side; comparing code with the saved file, git HEAD or another model; pop-out view moves | `history` test: 25 mixed steps undone give AA4's original bytes and redone give the edited file (I3); snapping and naming tests; screenshots against the mock bridge |
| `phase2/output-fix` | The bridge's output change built a subclass of `HeadlessWorkspace` that javac refuses (NetLogo's Scala traits give it abstract and default versions of the same methods), so `phase2/bridge-output` did not build. The bridge now makes the workspace silent and receives its output through NetLogo's own output mirroring | `OutputTest`, `WorkspaceTest`, `ExperimentTest` against NetLogo 7.0.4's jars (cloud, Linux) |
| `phase2/protocol-1` | Protocol 1 (Part B): snapshots, commands and settings during a run, speed as NetLogo's slider, view updates on ticks, stop conditions, the agents hook, plots, recompile with a dry run, experiment progress, agent state with links | 13 `ProtocolOneTest` tests and the updated bridge suite against NetLogo 7.0.4's jars; `logos-bridge-client` A1 to A11 tests against the real bridge (see "Part B") |
| `phase2/app-protocol-1` | The app on protocol 1: one bridge run per forever button with writes going straight to it, the speed slider and "on ticks" changed in place, "Pause when" as the stop condition, snapshots after commands, plots drawn, Apply without reopening (the world is kept; it asks only when breeds or variables would be lost), experiment steps while runs go on | 26 front-end tests; AA4 in the app against the real bridge in the cloud: setup, go, plots filling, speed at slider 15 giving about 1.9 s per tick, "Pause when ticks >= 50" stopping at 50, Apply of a new reporter keeping the world at tick 551, Apply removing a variable listing the loss |
| `phase2/protocol-tests` | B3 and B4 as bridge tests | Both pass against NetLogo 7.0.4's jars |

Not yet built: adding a stop condition or time limit to an experiment that has none, and making a first experiment in a model with none (both wait for an example of the element order NetLogo 7.0.4 writes); `.nls` files (the app doesn't open them yet, so search, autosave and watching cover the model file only); editing chooser choices (waiting for an example of how NetLogo 7.0.4 writes number, boolean and list choices); the D-18 layout command (waiting for NetLogo's minimum widget sizes); asking the operating system to confirm the user before unmasking a secret (K1).

## Part B: protocol 1

Measured 2026-10-12 in the cloud (Linux, OpenJDK 21), against NetLogo 7.0.4's own jars copied from Thomas's install. JUnit can't be downloaded there, so the bridge tests ran under a small stand-in for JUnit's runner; on Windows they run under Gradle as usual. All bridge tests passed except `extensionPrimitivesAppearWhenLoaded`, which needs the `nw` extension, not copied.

| # | Criterion | Result | Status |
|---|---|---|---|
| B1 | Snapshot after a command | The snapshot's view frame is `HeadlessWorkspace.exportView`, encoded as every view frame is; `snapshotsHaveRunZeroAndFollowACommand` checks the frames, their order and run number 0. No separate byte comparison yet | Met by construction; Windows check in the handoff |
| B2 | Commands during a run | A run with two commands queued during it ends in the same `exportWorld` as the same commands sent between `repeat` runs (`commandsDuringARunMatchTheSameStepsSentBetweenIterations`, a random-walk model); AA4 with slider changes during a run completes (`aa4CommandsDuringARunDontBreakIt`) | Met for the test model; the AA4 sequence with three sliders and a Command Center command is in the handoff |
| B3 | Observation doesn't change runs | AA4 seed 7, 100 iterations, with plots, 20 snapshots, view and agent frames every iteration, an observe stream and a stop condition: same `exportWorld` as an unobserved run with plots on | Met |
| B4 | Plot data matches NetLogo | After 100 iterations, every pen's points equal `export-all-plots` for AA4, Wolf Sheep Predation, Virus and Ants (Models Library, Sample Models/Biology) | Met |
| B5 | Apply | NetLogo's Check (netlogo-gui `CompilerManager` at v7.0.4) never clears the world: it reallocates it, so variables keep their values by name, removed variables lose theirs and agents of removed breeds die. `workspace.recompile` does the same, and the dry run lists what would be lost; tested for a changed procedure body, a new global, a removed global and a removed breed | Met; comparing with the GUI by hand is in the handoff |
| B6 | Speed | The pacing follows NetLogo's tick-based update policy (`UpdateManager.scala`): 30 ticks took 1.0 s at the middle of the slider (frame rate 30) | Needs the hand measurement against the GUI (handoff) |

Plot code runs on the plot manager's own random number generator, a clone of the main one made when the workspace is created, read from NetLogo 7.0.4's jar and confirmed by `plotCodeDoesNotChangeTheMainRandomNumbers`. That settles PROTOCOL.md's open point.

## Spike check in the cloud

Check of the spike itself, 2026-10-11, in the cloud (Linux, WebKitGTK 2.52 under Xvfb with software rendering, `--synthetic`, so no NetLogo and no GPU). These numbers check that the spike works; they are not criterion measurements. Every transport delivered all 324 view frames and about 30 agent updates per second with no gaps. Median latency from Rust to the handler was about 5 ms for the channel and polling and 1.5 ms for the WebSocket. Main-thread time per drawn frame was at most 2 ms at the 95th percentile, except when all 10,000 turtles were also drawn as rectangles on the canvas (15 to 17 ms median). WebKit rounds `performance.now()` to 1 ms, so times below that read as 0.
