# Logos decision log

Each entry records a decision, the reason for it, and what would make us revisit it. Newest entries go at the bottom. Entries are numbered so code comments and PRs can refer to them (for example "per D-07").

Status values: **accepted**, **proposed** (waiting on Thomas), **superseded**.

---

## D-01 Licensing split — accepted

- The Rust workspace, protocol schema, Python client and documentation are licensed **MIT OR Apache-2.0** (the usual Rust dual license).
- The JVM bridge (`bridge/`) is licensed **GPL-3.0-or-later**.

Reason: the bridge loads NetLogo's jars into its own process, so it is a combined work with NetLogo. NetLogo is "GPL version 2 or any later version". The `nw` extension bundles Gephi, which its authors license as GPLv3 only, so a bridge that runs `nw` models has to be GPLv3. The Rust side talks to the bridge only over IPC and never links NetLogo code, which keeps it free to be permissive.

Rules that follow from this:

- No NetLogo source may be copied or translated into the permissive parts of the repo. This includes the lexer (`TokenLexer.scala`) and the structure grammar (`StructureCombinators.scala`). We may read them to understand behavior, but our parser is written from the language documentation and from tests against the real engine.
- Primitive tables (names, arity, argument types) are produced at run time by the bridge from the user's installed NetLogo, not checked into the permissive code.
- Models in the test corpus keep their own licenses. Models Library and CoMSES models are referenced by manifest and downloaded or read from the local install. They are not committed.

This is a working position, not legal advice. Have it reviewed before the first tagged release.

## D-02 Public repository from the start — accepted

## D-03 "API" means two things — accepted

1. A programmatic API for driving Logos (local service plus a Python client), sharing one capability layer with the MCP server.
2. LLM provider support for the in-app assistant.

## D-04 Assistant providers at launch — accepted

Anthropic, OpenAI and Ollama. Users supply their own keys. A provider trait in the core keeps adding others cheap.

## D-05 Reference engine is NetLogo 7.0.4 — accepted

Thomas's installed version. The Windows install bundles a Java 17.0.2 runtime, and the classes in `netlogo-7.0.4.jar` are compiled for Java 11 (class file version 55). NetLogo's `main` branch has since moved to Java 21, so `main` is not a reference for 7.0.4 behavior; read the `v7.0.4` tag instead. The bridge is compiled with `--release 17` so it runs on the runtime NetLogo already ships, and users don't need a separate Java install.

Every project records the engine version it was run on, and the bridge refuses to run a project against a different version unless the user confirms. Later versions (7.1.x) are added as additional reference engines, not replacements. Known reason to pin: an `nw` model hangs at step 0 in BehaviorSpace on 7.1.0 but runs on 7.0.4.

## D-06 Legacy `.nlogo` files follow NetLogo 7.0.4's behavior — accepted

Opening a `.nlogo` shows a warning and converts it to `.nlogox`. Logos does the conversion by asking the bridge to open the file in NetLogo and save it in the new format, so the result is whatever NetLogo itself produces. Logos does not write its own `.nlogo` parser. The original `.nlogo` is never overwritten.

## D-07 Compile diagnostics come from NetLogo's compiler first — accepted

The Rust parser provides structure for editor features (outline, go-to-definition, references, rename, occurrence highlighting, word-wise editing). Errors and warnings in Phase 1 come from compiling through the bridge. The Rust semantic pass takes over diagnostics only where the conformance tests show it agrees with NetLogo.

Reason: NetLogo parsing depends on breed-generated primitives, extension primitives and arity. Using the real compiler means diagnostics are correct from the first release.

## D-08 Analysis lives in Python at first — accepted

The Rust core generates experiment designs (full factorial, LHS, Sobol, Morris sample matrices), schedules runs and writes Parquet. Sensitivity indices, replicate checks and calibration are done through the Python client on top of SALib and existing ABC/optimization libraries. Revisit if the Python dependency becomes a real problem for users.

## D-09 One JVM, many workspaces — accepted

The bridge hosts many `HeadlessWorkspace` instances in one process, each on its own thread and addressed by ID. NetLogo documents that headless workspaces run independently on separate threads. Parallel experiments use one bridge per machine (or a few, if one JVM's heap becomes the limit), not one JVM per run.

## D-10 Live view starts with rendered frames — accepted

NetLogo's headless renderer can return the view as an in-memory image (`HeadlessWorkspace.exportView`). Phase 0 compared streaming those frames with streaming agent and patch state for rendering in Rust. The view starts with rendered frames: they match NetLogo's own view exactly and need little code. Agent-state streaming is used for inspection and interaction, and for views NetLogo cannot draw.

Spike A results on Windows (docs/phase-0-results.md): rendered frames on AA4 at 28.7 fps shown against a 30 fps cap (A4); agent state for 10,000 turtles at 30.2 updates/s against the same cap, and 219 updates/s uncapped (A5). Accepted by Thomas, 2026-10-09.

## D-11 Parser approach: hand-written lossless parser — proposed

A hand-written lexer and parser in Rust that produces a lossless syntax tree (every byte, including whitespace and comments, belongs to a token), probably using `rowan`. Tree-sitter may be added later as a separate grammar for highlighting in editors that use it. Spike B confirms.

## D-12 `gis` and `nw` are in scope for planning — accepted

Both are in the conformance corpus from Phase 0, starting with the Models Library examples `Code Examples/Extensions Examples/gis/GIS General Examples` and `Code Examples/Extensions Examples/nw/NW General Examples`. They run on the reference engine as they are. A native engine (Phase 4) does not need to support them.

## D-13 Name — accepted

"Logos", a reference to Logo, the language NetLogo descends from.

## D-14 Reading values never touches the model's random number generator — accepted

In 7.0.4, `HeadlessWorkspace.command` and `HeadlessWorkspace.report` both run with the world's main random number generator (`world.mainRNG`). Any reporter that iterates an agentset, such as `mean [energy] of turtles`, shuffles the agentset and consumes random draws. If Logos polled monitors, plots or the inspector through `report` during a run, the run would change.

NetLogo avoids this in two ways, and the bridge copies both:

- The GUI runs monitors and sliders with the separate `auxRNG`.
- BehaviorSpace (`lab/Worker.scala`) runs metric reporters with a clone of the main generator (`mainRNG.clone`), so measuring has no effect on the run.

The bridge evaluates every observation (monitors, metrics, inspector reads, live-view state queries) with a job owner holding `mainRNG.clone`, matching BehaviorSpace. `workspace.command` is the only path that is allowed to advance the main generator. A conformance test checks that a run with observation every tick gives the same results as a run with no observation.

Confirmed on NetLogo 7.0.4 with AA4-Combined (`run-seed` 7): reading `mean [age] of households`, `one-of households` and `n-of` every step through the main generator changes the population at year 840 from 20 households to 17. With the cloned generator, the observed and unobserved runs match exactly. `WorkspaceTest.observingDoesNotChangeTheRun` in the bridge guards this.

## D-15 Primary test model: AA4-Combined — accepted

Thomas's modified Artificial Anasazi model (from Stonedahl & Wilensky 2010, Models Library) is the main model for Phase 0 and the first entry in the conformance corpus. It lives at `corpus/own/aa4-combined/` with the `data/` files it reads at setup, copied from the 7.0.4 Models Library.

Properties that matter for testing:

- Seeded with `random-seed run-seed` right after `clear-all`, so runs are reproducible by design.
- Reads four data files with `file-open` relative to the model's folder. The bridge must open it by path (`workspace.open`), not from a string, so NetLogo sets the model's directory.
- Uses no extensions. `gis` and `nw` are tested with the Models Library examples `GIS General Examples` and `NW General Examples`.
- Contains eight BehaviorSpace experiments, which give Phase 1 a direct comparison: the Logos experiment runner should reproduce what NetLogo's own headless BehaviorSpace writes for the same experiment.
- Stops itself when `year > 1350`.

License: the original model's license and Thomas's modifications both need to be stated before the file is committed to the public repository.

## D-16 Jupyter notebooks inside Logos — proposed

Thomas suggested integrating `.ipynb` notebooks into the Logos interface, so analysis sits next to the model and its experiments.

This follows from D-08 (analysis lives in Python). Points to settle before building it:

- Notebooks run on a standard Jupyter kernel (`ipykernel`) started by Logos, using the Jupyter messaging protocol. Logos does not write its own kernel.
- The kernel has the Logos Python client loaded and connected to the running project, so a notebook can read experiment results (Parquet) and provenance, and start runs.
- Notebooks are ordinary `.ipynb` files in the project folder, so they also open in Jupyter or VS Code.
- Placement: after the Python client exists (Phase 2), in the desktop app (Phase 3). Nothing in Phase 0 or 1 depends on it.

## D-17 Launch commands are built as argument lists — accepted

NetLogo 7.0.4's `netlogo-headless.bat` fails when NetLogo is installed in a folder with a space in its name, such as `C:\Program Files\NetLogo 7.0.4` (Java reports `Could not find or load main class Files\NetLogo`). The script sets `BASE_DIR` with a trailing backslash and passes `-Dnetlogo.docs.dir="%BASE_DIR%"`; the argument then ends in `\"`, which the Windows command-line parser reads as an escaped quote, so the string never closes and the following arguments run together.

Logos never builds a launch command by joining strings or by calling NetLogo's launch scripts. It starts Java directly and passes each argument as a separate list element (`std::process::Command::arg`), as `logos-bridge-client` already does. The fix for NetLogo's script (drop the trailing backslash in that one place) is a candidate upstream PR.

## D-18 Converting a legacy model does not re-lay out its widgets — proposed

When NetLogo 7.0.4's GUI opens a model saved before NetLogo 6.4.0, it asks whether to resize the interface widgets for NetLogo 7's larger widgets ("Resize and adjust positions" or "Skip", `app/FileManager.scala`). "Resize and adjust positions" runs `WidgetPanel.convertWidgetSizes`, which takes each widget's minimum size from its Swing component on screen and then moves widgets apart to keep their gaps. The same tool is in the Tools menu (Convert Widgets to New Sizes), so it can also run after the model is open.

`model.convertLegacy` matches the GUI with "Skip": the widgets keep their old geometry and are written with `sizeVersion="0"`, which NetLogo 7 reads as old-size widgets. The bridge cannot reproduce "Resize and adjust positions" exactly without drawing the widgets in Swing.

Proposal: the conversion stays as it is. Re-laying out old-size widgets becomes a separate, optional command in Logos's interface editor (Phase 2), where Logos knows its own widget sizes. Until then a user who wants NetLogo's re-layout can open the converted model in NetLogo and run Tools > Convert Widgets to New Sizes.
