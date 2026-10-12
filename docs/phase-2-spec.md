# Phase 2: specification

Phase 2 is the Logos desktop app (D-21). A NetLogo user opens a `.nlogox` and sees the model as NetLogo shows it: the Interface, Info and Code tabs, the view, and the model's own widgets. From the same window the user edits code with NetLogo's diagnostics, edits the interface, inspects agents, makes the folder a project, sets secrets, and runs BehaviorSpace experiments on parallel workspaces.

The app sits directly on the Phase 1 crates (`logos-nlogox`, `logos-syntax`, `logos-lsp`, `logos-bridge-client`, `logos-experiment`, `logos-project`, `logos-secrets`) and the bridge. The programmatic layer (API, Python client, MCP server) is Phase 3 and the app doesn't depend on it.

The source for this specification is Thomas's design, `desktop-design.md`, with its wireframes and flows. The design is kept in the Project and is not committed to the repository. Section and gap numbers below (3.2, G4) refer to it. Decisions D-22 to D-29 record what Thomas settled from the design's open questions.

Engine: NetLogo 7.0.4 (D-05). Test models: AA4-Combined (D-15), MASTOC-LLM, and Models Library models named per criterion. Platforms: Windows first; macOS and Linux must build, start and pass the criteria marked for them.

The work is split into ten parts. Part F (framework spike) comes first because every screen depends on its outcome. Part B (bridge and protocol) comes next because the model screen needs it. Each part lists its scope and its success criteria. The PR list at the end orders the work so that Thomas can open and run a model in the app early, at step 7.

---

## Rules that hold throughout

These come from accepted decisions and apply to every part.

- **The model file stays a NetLogo file (D-19).** Every code, info, widget and experiment edit goes through `logos-nlogox` span edits. Layout, panel state, autosaves, Logos-only experiment settings and the activity log are stored outside the `.nlogox`.
- **Reading never changes a run (D-14).** Monitors, plots, inspectors, watches, hovers and pause conditions read through `workspace.observe`, the agent-state stream or snapshots. Only user actions that write (buttons, sliders, Command Center, inspector edits) use the main random number generator, through `workspace.command`.
- **Diagnostics come from NetLogo's compiler (D-07)**, through `logos-lsp`.
- **The view is NetLogo's own rendering (D-10).** Agent state is for hit-testing, inspection and the text alternative.
- **Secrets (Part K of Phase 1).** Values never reach files, logs, crash reports or the activity log.
- **Licensing (D-01).** The app is MIT OR Apache-2.0. Every UI dependency's license is checked and listed in the PR that adds it. Nothing from NetLogo's source goes outside `bridge/`.
- **Paths with spaces (D-17).** Processes start with argument lists.
- **Keyboard and screen reader.** Every control has an accessible name, and every action is reachable from the command palette and the keyboard. This is checked at the end of each screen's step, not only at the end of the phase.

---

## Part F: framework spike

### Scope

The design recommends Tauri 2 with a web front end and CodeMirror 6 (design 5.4), conditional on the live view meeting the Phase 0 rates inside a Tauri window on Windows. Spike F1 measures that before any screen is built.

- A Tauri 2 window under `spikes/f1-tauri/` (throwaway, like `spikes/view-window`). The Rust side starts the bridge with `logos-bridge-client`, runs AA4 with view frames, and runs the generated 10,000-turtle model from A5 with agent state.
- Three transports from Rust to the front end, each measured separately:
  1. a Tauri channel (`tauri::ipc::Channel`) pushing each frame;
  2. `tauri::ipc::Response` returned to a request the front end makes for the latest frame;
  3. a binary WebSocket from the Rust host on `127.0.0.1`.
- Frames are drawn with a 2D canvas (`putImageData`). If 2D canvas misses the target where the transport doesn't, a WebGL texture upload is measured as well.
- The front end reports frames received, frames drawn, and the main thread's busy time per frame (from `performance` marks and long-task entries).

The spike is repeated on Linux (WSL with WSLg, and a native Linux machine with an NVIDIA GPU if one is available) and on macOS when a Mac is available.

If no transport meets the target on Windows, the fallback is to draw the view in a native child surface inside the Tauri window, measured as a second spike. Electron stays available as a later branch of the front end, since the front-end code would carry over (D-29). egui for the whole app is the last option.

### Success criteria

| # | Criterion | Target |
|---|---|---|
| F1 | AA4 view frames in a Tauri window, Windows | At least 27.3 fps drawn at a 30 fps cap (no more than 5% below A4's 28.7 fps), over at least 300 frames |
| F2 | Agent state for 10,000 turtles, Windows | At least 28.7 updates/s decoded at a 30 fps cap (no more than 5% below A5's 30.2) |
| F3 | Room for the rest of the app | During F1 and F2, the front end's main thread is busy less than 16 ms of each 33 ms frame, at the median and the 95th percentile |
| F4 | Linux and macOS | F1 to F3 measured and reported; a miss there is written up and does not block the decision |

Thomas chooses the framework after seeing F1 to F4. The choice is recorded as an accepted decision (D-29 becomes accepted, or a new entry replaces it).

---

## Part B: bridge and protocol

### Scope

Protocol version 1. Each item is one PR touching `protocol/PROTOCOL.md`, `crates/logos-protocol`, `crates/logos-bridge-client` and `bridge/` together. The client and bridge check the version in `engine.hello` as they do now.

- **Snapshots (G15).** `workspace.snapshot` returns a view frame and an agent-state frame for a workspace that is not running, as binary frames with run number 0. When the client asks for it, the bridge also sends a snapshot after each `workspace.command`, so the view updates after `setup`, a once-button, a Command Center command or a slider change. A run in progress answers `workspace.snapshot` between two iterations.
- **Agent state, version 1.** The Phase 0 layout (PROTOCOL 6.2) is replaced. The new layout adds links (end points, color, thickness, shape index, breed index, hidden) and a layout version byte, and keeps turtle and patch columns as they are. It is still read without the random number generator.
- **Commands during a run (G1).** `workspace.command` during a `workspace.run` is queued and runs between two iterations, on the main random number generator, in arrival order. Its response is sent when it has run. This is how sliders, switches, choosers, input boxes, once-buttons, the Command Center and inspector edits write while a forever button runs. The results of a run with queued commands match the same commands sent between `repeat 1` runs.
- **Run settings during a run (G2).** `workspace.runSettings` changes the delay between iterations (NetLogo's speed slider) and the view, agent and observe stream settings of a run in progress. The speed slider's mapping to a delay follows NetLogo's GUI at `v7.0.4` (read, not copied, D-01).
- **View updates on ticks.** A run's view stream can send frames per iteration (the default) or only when `ticks` changed since the last frame (NetLogo's "on ticks"). NetLogo's "continuous" updates, which redraw inside a procedure, are not offered (design Q3, see "Open items").
- **Plots (G4).** Before the protocol change, the PR settles PROTOCOL 9's open point: which random number generator NetLogo 7.0.4 uses for plot code in a headless workspace, read from the `v7.0.4` source and confirmed by a test. Then:
  - `workspace.plots` returns every plot's definition (name, axis ranges, autoplot, legend, pens with color, mode, interval and their points).
  - During a run, a `run.plots` notification sends each pen's new points since the last one, and events for plot clear and pen reset, after each iteration that changed a plot, at most at the view's frame rate. After a command, the same changes are sent with the snapshot.
  - Interactive runs have plot updates on, as in the NetLogo GUI. Experiments keep them off by default (Phase 1 Part E).
- **Apply (G5).** `workspace.recompile` installs new code in a workspace the way NetLogo's Check button does: it keeps the world when NetLogo would and clears it when NetLogo would, and its response says which happened. A dry-run form (`"dryRun": true`) answers whether applying would clear the world, without changing anything, so the app can warn first. Diagnostics while typing stay in the language server's check bridge (D-25).
- **Stop on a condition (G6).** `workspace.run` takes an optional `stopWhen` reporter, evaluated after each iteration through the observation path (D-14). The run ends with a new `run.finished` reason, `"condition"`.
- **Experiment progress (G11).** During `experiment.run`, the bridge sends `experiment.progress` notifications (run number, step) at most every 250 ms per workspace. A run's start is the first notification; its end is the existing response.
- **Hooks for agent forever buttons (D-26).** `workspace.run` takes an `agents` field: `{ "kind": "observer" }` in Phase 2, with `"turtles"`, `"patches"`, `"links"` and an optional breed reserved. The bridge rejects the reserved kinds with `-32602` in Phase 2. Run numbers already identify runs in every notification, so more than one run per workspace can be added later without changing message shapes. The `1005 Busy` rule for a second `workspace.run` stays in Phase 2 and is documented as the place that changes.

### Success criteria

| # | Criterion | Target |
|---|---|---|
| B1 | Snapshot after a command | After `setup` on AA4, the snapshot's view frame equals `HeadlessWorkspace.exportView` at the same point, byte for byte |
| B2 | Commands during a run | AA4 seed 7: a run of 100 iterations with three slider changes and one Command Center command queued at given iterations ends in the same world (`exportWorld`, compared as text) as the same sequence sent between `repeat 1` runs |
| B3 | Observation doesn't change runs, with plots and snapshots | A12 repeated with plots streamed, a snapshot every iteration, agent state every iteration and a `stopWhen` reporter: identical to an unobserved run with plots on |
| B4 | Plot data matches NetLogo | For AA4 and three Models Library models with several pens (named in the step's PR), the points received after 100 iterations equal NetLogo's `export-all-plots` output from the same workspace at the same point |
| B5 | Apply | For a set of edits (procedure body only; new global; new breed), `workspace.recompile` keeps or clears the world as NetLogo 7.0.4's GUI does when Check is pressed, and the dry run predicts it correctly |
| B6 | Speed and delay | With the delay set to its maximum, AA4 runs at the iteration rate NetLogo's GUI shows at the same slider position, within 10% (measured by hand in the GUI and recorded) |

---

## Part A: app shell

### Scope

- **Crates and folders.** `crates/logos-app` (the Tauri host, depending on the core crates) and `app/` (the front end). The front end is TypeScript with a small UI library chosen in the step's PR, with its license listed. No server and no network port except the F1 transport if a WebSocket is chosen, bound to `127.0.0.1` with a per-session token.
- **Launcher** (design 2.1, 3.1): recent projects, open a model or folder, convert a legacy `.nlogo` (D-06), open a Models Library example, engine status.
- **First launch** (design 3.1): find NetLogo in the order in `repo-layout.md` ("Finding NetLogo"), list installs with versions, select 7.0.4 when found; otherwise explain, link to NetLogo's download page and offer "Choose folder…" (design Q6, see "Open items"). Start the bridge and show `engine.hello`, or the bridge's log lines and the path tried.
- **Bridges** (D-25). One run bridge per engine for the whole app, and the language server's check bridge (`-Dorg.nlogo.noGenerator=true`). Both get explicit heap limits set in the app's settings, with defaults chosen from F1 and step 7 measurements so that both fit with the OS on a 16 GB machine. Both exit when the app exits or crashes (stdin closes, A10).
- **Project window regions** (design 2.2): menu and title bar, activity bar (Model, Files, Search, Experiments, Results; LLM and Git entries are Phase 3), side panel, editor area with tabs and one split, right panel (Inspectors; room for the assistant), bottom panel (Command Center, Output, Problems, Log, Activity), status bar.
- **Command palette** with every action, and rebindable shortcuts.
- **Settings**: user settings in the app data folder, project settings in the app data folder keyed by project path (D-19, D-24); a searchable form with the JSON source available.
- **Themes**: light and dark, following the system by default, and font size. The color system is designed in this step; the wireframes are grayscale.
- **Activity log** (G13, the log part): every action that changes a file or the running model, with time and actor ("user" in Phase 2), shown in the Activity panel and stored in the app data folder.
- **Logs and crash reports** go to the app data folder and never contain secret values.

### Success criteria

| # | Criterion | Target |
|---|---|---|
| A1 | Start time | Release build on Thomas's machine: launcher shown within 2 s of start; AA4 Interface tab with its first view frame within 5 s of choosing it (A2 took 2.91 s to the first reporter) |
| A2 | No bridge left behind | Closing the app, and killing its process, leaves no run or check bridge running after 3 s, on Windows, Linux and macOS |
| A3 | Engine discovery | With NetLogo 7.0.4 in `C:\Program Files\NetLogo 7.0.4`, in a folder chosen by the user, and missing: each case shows the right screen and the bridge starts in the first two |

---

## Part M: model document

### Scope

The model document (design 2.4, 3.2) with NetLogo's three tabs.

- **Typed widget model.** A layer over `logos-nlogox` that reads every NetLogo 7.0.4 widget kind (view, button, slider, switch, chooser, input box, monitor, plot with pens, output, note) with its attributes and geometry, and writes changes back through span edits. Unknown widget kinds and attributes are kept and shown as unknown.
- **Interface tab, running.** Widgets drawn at their saved positions. The view widget shows frames and snapshots. Buttons: observer once-buttons through `workspace.command`; observer forever buttons through `workspace.run`; once-buttons of turtle, patch and link kind through `workspace.command` with the button's commands wrapped in `ask`, as NetLogo does for once-buttons. While a forever button runs, other forever buttons are disabled with a note saying why (D-26). Sliders, switches, choosers and input boxes write through `workspace.command` (G1). Monitors read through the run's `observe` stream during a run and through `workspace.observe` after each command, with NetLogo's decimal places. The toolbar has the speed slider, view updates (per iteration or on ticks), and the run strip (tick counter, Step one tick, Pause, run state).
- **Plots** drawn from Part B's plot data, with NetLogo's pen modes (line, bar, point), autoplot and legends.
- **Command Center** with observer, turtles, patches and links targets, history, and output; **Output** panel for `workspace.output`.
- **Info tab**: rendered Markdown, with an Edit toggle and live preview. The rendering follows NetLogo's Info tab Markdown where the two differ; differences found are listed in the results.
- **Code tab**: CodeMirror 6 connected to `logos_lsp::server::serve` in the app process through an in-memory connection, with every Phase 1 Part S feature: diagnostics, outline, definition, references, hover, completion, selection ranges, rename (including widgets and experiments). `.nls` files open as code documents with the same server. Apply (and its shortcut) uses `workspace.recompile`, warns first when the dry run says the world would be cleared (D-23), and the status bar shows "Code changed, not applied" until then.
- **Inspector and watches** (design 4.4): clicking an agent in the view (hit-testing on agent state) opens an inspector in the right panel with its variables (names from `engine.primitives` built-in variables and `workspace.program`, read after each compile and kept; values from `observe`), and a small view centered on the agent, cut from the frame. Editing a value writes through `workspace.command`. Watches record a reporter every tick and show a sparkline. "Pause when" uses `stopWhen` (G6).
- **Pop-out view** (D-22): the same frames shown in a dockable panel and in a separate window; the bridge sends each frame once.
- **Text alternative**: "Describe view" and a screen reader panel with tick, agent counts by breed and the agent under a keyboard cursor that arrow keys move over patches.
- **Preset layouts** (design 2.4): *NetLogo* and *Code and run* in Phase 2; *Experiment* with Part E; *GABM* in Phase 3. Users can save their own.

### Success criteria

| # | Criterion | Target |
|---|---|---|
| M1 | View in the app | AA4 with view updates per iteration at a 30 fps cap: at least 27.3 fps shown, over at least 300 frames, release build on Thomas's machine |
| M2 | Reading doesn't change the run, through the app | AA4 seed 7, 200 ticks, with every monitor and plot shown, an inspector open on a household, two watches and a "Pause when" that never fires: final world identical to a run of the same length through `logos-bridge-client` with no observation (B3, through the app's path) |
| M3 | Writes during a run | Moving a slider during a run of AA4 changes the global before the next iteration (checked with a watch on the global and the tick it changed) |
| M4 | Diagnostics in the app | Typing in AA4's Code tab at 60 ms per key, NetLogo's diagnostic is drawn within 500 ms of the last keystroke (S5), three runs, release build |
| M5 | Every widget kind | The Models Library models named in the step's PR, covering every widget kind, open with every widget drawn at its saved position and size; monitors, plots and buttons work as in NetLogo for one minute of running, checked by Thomas against the NetLogo GUI |

---

## Part I: interface editing, undo and recovery

### Scope

- **Element edits in `logos-nlogox` (G8).** Insert, delete and reorder elements (widgets, plot pens, experiments and their children) with span edits that leave every other byte unchanged, using the file's own indentation and line endings.
- **Widget sizes and layout (D-18).** A table of Logos's own minimum widget sizes, and the layout command for old-size widgets (`sizeVersion="0"`): it enlarges each widget to its minimum and moves widgets apart to keep their gaps, then sets `sizeVersion` as NetLogo 7.0.4 does. The command shows the result for review before applying.
- **Interface edit mode** (design 4.3): select, move, resize and delete widgets, the Add menu with every widget kind, and property sheets with NetLogo's fields for each kind. Keyboard alternatives for moving and resizing.
- **World settings**: world size, patch size, wrapping, tick counter label and the other model settings NetLogo's Settings dialog has, written through `logos-nlogox`.
- **Undo and redo** across code, info, interface and experiment edits in one history per model, with grouped steps. Each step is labeled with its actor (design principle 6), so Phase 3 can add the assistant without changing the history.
- **Autosave and crash recovery** (D-24): unsaved buffers are copied to the app data folder every 30 seconds and on focus loss; after a crash, opening the model offers to restore them, with a diff against the file on disk.
- **Outside changes**: the app watches the model, `.nls` files, `logos.toml` and the experiment settings file. A change on disk to a file with no unsaved edits reloads it; a change to a file with unsaved edits asks which to keep.

### Success criteria

| # | Criterion | Target |
|---|---|---|
| I1 | Edits stay NetLogo files | For AA4 and 20 corpus models: one code edit, one info edit, adding one widget of each kind, moving one widget and deleting one widget, each saved separately. Every result opens in NetLogo 7.0.4 without a warning, and the byte diff against the original covers only the edited element (B2, B3) |
| I2 | Layout command | For the A9 legacy models converted with `model.convertLegacy`: no widget smaller than Logos's minimum, no two widgets overlapping, and each file opens in NetLogo 7.0.4. Thomas compares five of them with NetLogo's Tools > Convert Widgets to New Sizes and records the differences |
| I3 | Undo | A sequence of 20 mixed code, interface and info edits undone fully gives a file byte-identical to the original |
| I4 | Recovery | Killing the app with unsaved edits in code, interface and info, then reopening, restores all three |
| I5 | Outside changes | A change made by another program is noticed within 2 s |

---

## Part P: project screens

### Scope

- **Make project** (design 3.9): the review table from `logos project init` (path, guessed role, where it was detected, checkbox), detected secrets, the Python interpreter found; confirming writes `logos.toml`.
- **Files activity**: the project tree with each file's role from `logos.toml`.
- **Project document**: a form over `logos.toml` that keeps unknown keys, with a "View source" toggle.
- **Data files**: a table preview of `input` and `output` files, with the data dictionary beside it, and a form editor for `*.dictionary.toml`.
- **Checks**: `logos check` runs when a project opens, after saves that change its inputs, and before experiments; results go to the Problems panel and the status bar, each linked to the file or name involved.

### Success criteria

| # | Criterion | Target |
|---|---|---|
| P1 | Same as the CLI | For AA4 and MASTOC-LLM, the review shows the same files and secrets as `logos project init --dry-run`, and the confirmed `logos.toml` is identical to what the CLI writes for the same choices |
| P2 | Checks in Problems | Each problem P3 of Phase 1 reports (missing input, missing output folder, invalid dictionary, missing Python or package, unset secret, compile error) appears in Problems with a working link |

---

## Part K: secrets screen

### Scope

The Secrets document (design 3.7) over `logos-secrets`: declared names with required or optional, set or missing, masked value and last change; Set, Change, Delete (with confirmation), Unmask and Copy; import from `.env` (Phase 1 K3).

Unmask asks the operating system to confirm the user: Windows Hello (or the account password through Windows' credential prompt) on Windows, Local Authentication on macOS, and on Linux a confirmation in Logos after the keyring is unlocked. The value shows for 30 seconds and hides again; a copied value is cleared from the clipboard after 30 seconds if the clipboard still holds it. Every unmask and copy is logged by name and time in `secrets-access.log`, as the CLI does.

### Success criteria

| # | Criterion | Target |
|---|---|---|
| K1 | Unmask and copy | On Windows: unmask asks for Windows Hello or the password, hides after 30 s; copy clears the clipboard after 30 s; both are in the access log with name and time, without the value |
| K2 | No secret in any file the app writes | With a known fake key set, unmasked and copied: no match for the value in the app data folder (settings, logs, crash reports, autosaves, activity log) or the project folder |
| K3 | macOS Keychain | Phase 1 K1 and this part's K1 run on a Mac when one is available; until then, listed as pending |

---

## Part E: experiments

### Scope

- **Experiment settings file (D-27).** Logos-only settings (workers, plot updates, output formats; later the Part L settings) live in a TOML file next to the model, `<model name>.experiments.toml`, with one table per experiment name. It is listed in `logos.toml` with role `config` when the folder is a project, and works without a project. Unknown keys are kept. The file has a format version so later versions, or tools that read BehaviorSpace setup files, can be matched to it.
- **Experiment editor** (design 3.3): the experiments in the model and in separate BehaviorSpace XML files; every BehaviorSpace setting in BehaviorSpace's order, with the run count computed live; metrics and conditions checked by the language server; the Logos-only section from the settings file; footer with total runs, estimated time from earlier runs, the `logos check` result and Run. Edits to experiments in the model go through Part I's element edits.
- **Run dashboard** (design 3.4): state, done / failed / remaining, elapsed and estimated time, Pause new runs, Stop, Resume; one row per worker with run number, parameter values, step (G11); the runs table; failed runs with `ErrorData` and a link to the code position. Closing the dashboard leaves the experiment running, with progress in the status bar. An interrupted experiment offers Resume when the project opens (Phase 1 E4).
- **Results** (design 3.4): the Parquet table with filters by parameter, and three plot types (metric over steps, metric against a parameter, one line per run or mean and spread over repetitions). "Open in notebook" is Phase 3 (D-16).
- **Run records**: the provenance fields of a run as a document, reachable from the runs table, Problems and the command palette.

### Success criteria

| # | Criterion | Target |
|---|---|---|
| E1 | Same results as the CLI | AA4 `N3-limit1` run from the app with 8 workers: table output identical to NetLogo's reference table, Parquet equal to it (Phase 1 E1, E2), and every run record complete (E3) |
| E2 | Throughput | The same run takes no more than 5% longer than `logos run` with 8 workers on the same machine (release builds) |
| E3 | Resume from the app | Killing the app after 3 runs and resuming from the dashboard gives the same table as an uninterrupted run |
| E4 | Experiment edits stay NetLogo files | Adding a varied variable, a metric and a new experiment in the editor: the model opens in NetLogo 7.0.4 and NetLogo's BehaviorSpace shows the same settings |
| E5 | Progress | The dashboard's step for each worker is never more than 1 s behind the bridge's `experiment.progress` |

---

## Part R: search

### Scope

Search and replace across the model's code, `.nls` files, info and widgets, with whole-word and case options and a preview of every replacement. The Git panel and the `.nlogox` diff move to Phase 3 (D-28). In Phase 2 the status bar shows the git branch and a count of changed files.

### Success criteria

| # | Criterion | Target |
|---|---|---|
| R1 | Replace stays lossless | Replacing a word across AA4 (code and widgets) changes only the matches; the file opens in NetLogo 7.0.4 |

---

## Part X: platforms and accessibility

### Scope

The checks that span every screen, run at the end on all platforms.

### Success criteria

| # | Criterion | Target |
|---|---|---|
| X1 | Keyboard only | Flows 1 to 4 of the design (open and make a project, edit code and apply, run and inspect, set up and run an experiment) and the secrets part of flow 5 can be completed with the keyboard alone, on Windows |
| X2 | Screen reader | With NVDA on Windows, every control in those flows is announced with its name and state; "Describe view" reads the view's summary. VoiceOver on macOS when a Mac is available |
| X3 | Linux and macOS | The app builds and passes A2, M1 (reported), I1 and E1 on Linux (WSL); macOS when a Mac is available |

---

## PR breakdown

Each step is one PR, or a few with one purpose each (AGENTS.md). Each step that needs Thomas's machine ends with a test handoff in the Project.

1. Docs: this specification, D-22 to D-29, and fixes to text made out of date by D-21.
2. Spike F1 (`spikes/f1-tauri`) and its measurement handoff. F1-F4. Thomas decides the framework.
3. Protocol 1: snapshots and agent state version 1 with links. B1.
4. Protocol 1: commands during a run, run settings, view updates on ticks, and the `agents` hook. B2, B6.
5. App shell: `logos-app`, `app/`, launcher, first launch, both bridges, project window regions, command palette, settings, themes, activity log. A1-A3.
6. Typed widget model in `logos-nlogox` (read and attribute writes).
7. Model screen, running: widgets, view, buttons, sliders and the other inputs, monitors, Command Center, Output, speed and view updates, Info tab rendered. M1, M3, part of M5. **Thomas can open and run models in the app from here.**
8. Plots: the plot generator question, protocol (`workspace.plots`, `run.plots`), plots in the app. B3, B4, rest of M5.
9. Code tab: CodeMirror on the in-process language server; Apply with `workspace.recompile`. B5, M4.
10. Inspector, watches, "Pause when" (`stopWhen`), hover, text alternative, pop-out view. M2.
11. Element edits in `logos-nlogox`; widget size table; layout command. I2.
12. Interface edit mode, world settings, Info editing, undo and redo across edits. I1, I3.
13. Autosave, crash recovery, watching for outside changes. I4, I5.
14. Project screens. P1, P2.
15. Secrets screen. K1, K2.
16. Experiment settings file and experiment editor. E4.
17. Protocol 1: experiment progress; run dashboard and resume. E1-E3, E5.
18. Results view and run records.
19. Search and replace; git branch and changes in the status bar. R1.
20. Platforms and accessibility pass; results written up. X1-X3, K3.

## Out of scope for Phase 2

- The in-app assistant, approval cards and connections from external agents (Phase 3; G12, G14).
- The LLM bridge editor, the LLM call log, recording and replay (Phase 3; D-20; G10).
- Jupyter notebooks and "Open in notebook" (D-16).
- The Git panel and the readable `.nlogox` diff (G9), the embedded terminal, and detaching panels other than the view (D-28).
- Forever buttons of turtle, patch and link kind, and more than one forever button running at once (D-26; the protocol hooks are in Phase 2).
- NetLogo's "continuous" view updates (design Q3).
- Downloading or installing NetLogo from the app (design Q6).
- Experiment designs beyond BehaviorSpace (Phase 3).
- 3D models.

## Open items

- **Design Q3**, continuous view updates: this specification assumes Thomas's agreement to the design's recommendation (offer per-iteration and on-ticks updates only, and explain the difference in the UI).
- **Design Q6**, NetLogo not installed: this specification assumes the design's recommendation (link to NetLogo's download page).
- **The framework** is decided after step 2.
- The Phase 1 items that remain open (the phase-done use of `logos run` results, K1 on macOS, the AA4 reference tables) don't block Phase 2.

## Phase 2 is done when

- Every criterion above is met on Windows, or the gap is written up in `docs/phase-2-results.md` with a decision about what to do instead.
- Thomas has used the app for his own work on one of his models (AA4 or MASTOC-LLM): opened it, edited and run it, and run one experiment from the app.
