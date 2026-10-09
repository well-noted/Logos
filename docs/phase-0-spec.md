# Phase 0 spikes: specification

Phase 0 answers two questions before any product code is written:

- **Spike A:** Can a separate JVM process run NetLogo 7.0.4 headless, drive it from Rust and stream a live view fast enough to use?
- **Spike B:** Can Logos read and write `.nlogox` files and NetLogo code without losing anything?

The spikes are independent and can run in parallel. Each one is split into small PRs (listed below) so that each PR has one purpose and can be reviewed alone.

Engine for all of Phase 0: NetLogo 7.0.4 (D-05).

---

## What we already know from NetLogo's source

These facts come from the `v7.0.4` tag of `NetLogo/NetLogo` and from Thomas's 7.0.4 install, unless noted.

- NetLogo 7.0.4 is built with Scala 3.7.0 and sbt. Its classes target Java 11, and the Windows install bundles a Java 17.0.2 runtime. NetLogo's `main` branch now targets Java 21, so `main` is not a reliable guide to 7.0.4.
- `HeadlessWorkspace.command` and `report` run with `world.mainRNG`. Reporters that iterate agentsets consume random draws, so reading values through `report` during a run changes the run. BehaviorSpace avoids this by running metrics with `mainRNG.clone`; the GUI uses a separate `auxRNG` for monitors and sliders. The bridge follows BehaviorSpace (D-14).
- The headless entry point is `org.nlogo.headless.HeadlessWorkspace`, with `newInstance`, `open(path)`, `openString(contents)`, `command(source)`, `report(source)` and `dispose()`.
- `HeadlessWorkspace.exportView` returns the rendered view as an in-memory `BufferedImage`. Headless runs can render frames without writing files.
- `export-world` output includes a `RANDOM STATE` section that saves the main random number generator (`world.mainRNG`). It does not save `auxRNG`, which only drives GUI widgets and does not affect model results.
- `parser-core` has a `TokenMapper` that lists every command and reporter name, and each primitive carries its syntax (argument types, return type, precedence). This is what the bridge will dump as the primitive table.
- `.nlogox` is read by `NLogoXMLLoader` and written through `NLogoXMLWriter`. The order of attributes in NetLogo's output is not alphabetical or fixed by element (for example `<view x=... wrappingAllowedX=... y=...>`). Our writer must keep the attribute order of the file it read, not imitate NetLogo's.
- NetLogo 7.1 changes how the code and info sections are written (always wrapped in newlines and `CDATA`) and adds `.nlm` module files. The reader must accept both the 7.0 and 7.1 forms.
- The NetLogo repository contains `.nlogox` test files (for example `test/fileformat/Wolf Sheep Predation.nlogox`) that are useful early fixtures.
- No tree-sitter grammar or language server for NetLogo exists. NetLogo issue #2090 asks for one; maintainers called a headless-backed language server feasible. The only VS Code support found (LogoScope) does syntax highlighting with regular expressions and splits hyphenated names.
- pyNetLogo's latest release (0.5.2, February 2024) says it supports NetLogo 6.1 and newer. It does not mention NetLogo 7. It is useful as a reference for driving headless NetLogo but not as a dependency.

---

## Spike A: JVM bridge and live view

### Scope

A Java 21 program (`bridge/`) that runs NetLogo's `HeadlessWorkspace` and talks to a Rust process. A throwaway Rust window (`spikes/view-window`) that shows a running model.

### Protocol (first version)

- Transport: the bridge's stdin and stdout, with each message framed as a 4-byte little-endian length followed by the payload. stderr carries logs only. This avoids sockets, ports and Windows firewall prompts.
- Control messages: JSON, in the JSON-RPC 2.0 request/response/notification shape, so they can be read in logs and written by hand in tests.
- Bulk messages (frames, agent state): binary, with a one-byte type tag, a workspace ID and a frame number, so they don't go through JSON.
- Methods for Phase 0: `engine.info`, `workspace.open`, `workspace.openString`, `workspace.command`, `workspace.report`, `workspace.dispose`, `workspace.run` (repeat a command N times or until stopped, streaming frames), `workspace.stop`, `engine.primitives` (primitive table as JSON), `model.convertLegacy` (.nlogo to .nlogox through NetLogo, D-06), `engine.shutdown`.
- All methods take a `workspace` ID except the `engine.*` methods.

Write the protocol down in `protocol/PROTOCOL.md` before writing either side.

### Two view approaches to measure

- **A-frames:** the bridge calls `exportView`, converts the image to raw RGBA (no PNG encoding), and sends it. The Rust window copies it to the screen.
- **A-state:** the bridge sends patch colors as one array and turtles as columns (who, x, y, heading, color, size, shape index, breed index, hidden). The Rust window draws patches as pixels and turtles as simple triangles. This approach doesn't need to match NetLogo's shapes in Phase 0. It measures throughput only.

### Success criteria

All measured on Thomas's Surface Studio, with numbers recorded in `spikes/view-window/RESULTS.md`.

| # | Criterion | Target |
|---|---|---|
| A1 | The bridge starts using NetLogo 7.0.4's own bundled Java runtime and jars, from an install path containing spaces | Works on Windows |
| A2 | Time from launching the bridge to the first `report` result on an opened model | Recorded; target under 5 s |
| A3 | Results match NetLogo: AA4-Combined's `N3-limit1` experiment (30 `run-seed` values × 2 `depletion-rate` values = 60 runs), each driven by the bridge as `setup` then `go` until the model stops | Every metric of every run identical to what `netlogo-headless.bat --experiment N3-limit1` writes |
| A4 | A-frames on AA4-Combined (80 × 120 patches at 4 px, a 324 × 484 view), running `go` until the model stops | At least 30 frames per second shown, with `go` throughput recorded with and without streaming |
| A5 | A-state with 10,000 turtles (Wolf Sheep Predation with the initial populations raised, or a generated test model) | At least 30 updates per second |
| A6 | 8 workspaces in one bridge process, each running AA4-Combined with `run-seed` 1 through 8 at the same time | Each workspace's outputs identical to running that seed alone; peak memory recorded |
| A7 | `NW General Examples` and `GIS General Examples` from the Models Library open, set up and run their example procedures in the bridge | No errors; outputs match NetLogo's GUI |
| A12 | Observation doesn't change runs (D-14) | AA4-Combined with all monitors and plot reporters read every tick gives the same A3 results as a run with no reads |
| A8 | `engine.primitives` returns the primitive table for core NetLogo plus loaded extensions | JSON with name, kind, argument types and return type for every primitive; `table:` and `nw:` primitives appear when those extensions are loaded |
| A9 | `model.convertLegacy` on a `.nlogo` file | Produces the same `.nlogox` as NetLogo 7.0.4's GUI conversion, compared after normalizing attribute order; never overwrites the source |
| A10 | Shutdown | Closing the Rust window or killing the Rust process ends the bridge process within 2 s; no orphaned `java.exe` |
| A11 | Errors | A compile error in `workspace.command` comes back as a structured error with message and source position; the bridge keeps running |

A4 and A5 decide D-10. If A-frames meets its target, the view starts with frames.

### Open technical questions for Spike A

- Is `exportView` safe to call from a thread other than the one running `go`, or does it need to run between ticks? It may need to run on the job thread between `go` calls.
- Plot update code runs inside the engine. Which random generator does it use in headless runs, and does it advance `mainRNG`? (Affects A12.)
- Do BehaviorSpace-style concurrent workspaces share any global state that matters (extension state, `file-*` working directory)? AA4-Combined's `file-open` calls during `setup` make this a direct test for A6.
- Does `HeadlessWorkspace.newInstance` under the bundled Java 17.0.2 runtime need any JVM options beyond those NetLogo's own launcher passes?

### PR breakdown

1. `protocol/PROTOCOL.md`: Phase 0 messages.
2. Bridge: process skeleton, framing, `engine.info`, `engine.shutdown`; Java tests without NetLogo.
3. Bridge: workspace open, command, report, dispose; engine tests against installed NetLogo.
4. `logos-bridge-client`: launch, supervise and shut down the bridge from Rust; framing; A1, A2, A10.
5. Bridge and client: `workspace.run` with A-frames streaming.
6. `spikes/view-window`: window that shows frames (`winit` plus `softbuffer` or `pixels`, chosen because they don't commit us to a UI framework). A4.
7. Bridge and viewer: A-state streaming. A5.
8. Multi-workspace test, A3, A6 and A12 comparison scripts. A3's reference output comes from running `netlogo-headless.bat --experiment N3-limit1` once and storing its table output under `corpus/`, keyed by engine version.
9. `engine.primitives` and `model.convertLegacy`. A8, A9.
10. `nw` and `gis` checks. A7. Write `RESULTS.md`.

---

## Spike B: lossless file format and code parser

### Scope

- `logos-nlogox`: read an `.nlogox` file into a structure Logos can edit, and write it back.
- `logos-syntax`: lex and parse NetLogo code into a lossless syntax tree that gives procedure and declaration structure.

### What "lossless" means

1. **Unmodified files:** reading and writing a file without changes produces a byte-identical file.
2. **Edited files:** changing one part (for example the code section or one widget property) changes only the bytes of that part. Attribute order, whitespace, line endings (CRLF or LF), `CDATA` sections, comments and elements Logos doesn't recognize are kept as they were.
3. **NetLogo accepts the output:** every file Logos writes opens and compiles in NetLogo 7.0.4 through the bridge.

### Design notes

- `logos-nlogox` parses with an event-level XML reader (for example `quick-xml`) and keeps the original byte span of every element and attribute. Edits replace spans. A file is never re-serialized from a model of its content unless it is being created from scratch.
- Unknown elements and attributes (from newer NetLogo versions) are kept and written back unchanged.
- `logos-syntax` is hand-written (D-11). The lexer handles NetLogo's identifier rules (hyphens, `?`, `!`, and other characters allowed in names), numbers, strings with escapes, `;` comments, brackets and literal lists. Every byte of input belongs to exactly one token, so joining the tokens reproduces the source exactly.
- The parser recognizes the top-level structure: `globals`, `breed`, `directed-link-breed`, `undirected-link-breed`, `<breed>-own`, `patches-own`, `turtles-own`, `links-own`, `extensions`, `__includes`, `to` / `to-report` ... `end`, and procedure parameter lists. Inside procedure bodies it records tokens and bracket nesting only. Full expression parsing needs the primitive table (from A8) and is Phase 1 work.
- The parser keeps going after errors (unbalanced brackets, missing `end`) and marks the error span, because the editor will be parsing incomplete code most of the time.
- Word-wise editing needs sub-word boundaries for hyphenated and underscored names. `logos-syntax` exposes a function that returns the segments of an identifier.

### Corpus for Phase 0

| Set | Source | Committed? |
|---|---|---|
| C1 | Every `.nlogox` in the NetLogo 7.0.4 Models Library, read from the local install | No (manifest lists paths and hashes) |
| C2 | `.nlogox` test files from the NetLogo repository | No (manifest lists URL and commit) |
| C3 | Thomas's own models, starting with AA4-Combined (`corpus/own/aa4-combined/`, D-15) | Pending license note |
| C4 | About 30 NetLogo models from CoMSES, including at least 5 using `gis` and 5 using `nw`, plus older `.nlogo` models converted through A9 | No (manifest lists URL and hash) |

C4 selection is a separate small task. Models should vary in size, NetLogo version of origin and use of `__includes`.

### Success criteria

| # | Criterion | Target |
|---|---|---|
| B1 | Unmodified round-trip over C1–C4 | 100% byte-identical |
| B2 | Edit round-trip: replace the code section with itself plus a comment line; change one slider's `max` | Diff limited to the edited region, for every corpus file |
| B3 | NetLogo accepts edited files (needs Spike A step 3) | Every B2 output opens and compiles in NetLogo 7.0.4 |
| B4 | Lexer is lossless | Joining tokens reproduces the code exactly, for every corpus model and every `.nls` include |
| B5 | Structure matches NetLogo | Procedure names, parameter lists, breeds, globals and `-own` variables match what the bridge reports for the same model, for every corpus model that compiles |
| B6 | Error recovery | A set of hand-written broken files (missing `end`, unbalanced brackets, unterminated string) parses without panicking and marks the right spans |
| B7 | Speed | Lexing and parsing the largest corpus model takes under 10 ms on Thomas's machine (release build) |

B5 depends on a bridge method that reports procedures and declarations. Until it exists, B5 is checked against a fixture recorded once from NetLogo.

### PR breakdown

1. `corpus/manifest.toml` format and `cargo xtask corpus fetch`.
2. `logos-nlogox`: span-preserving reader and unmodified writer. B1.
3. `logos-nlogox`: edit API for code, info and widget attributes. B2.
4. `logos-syntax`: lexer and identifier segmentation. B4, B7 (lexer part).
5. `logos-syntax`: top-level structure parser with error recovery. B5 (against fixture), B6, B7.
6. Engine check of edited files and live B5 comparison, once Spike A step 3 has merged. B3, B5.

---

## Out of scope for Phase 0

- Choosing the desktop UI framework. Spike A's viewer intentionally uses a minimal window library.
- Full expression parsing, type checking and language server features (Phase 1).
- The experiment runner, Parquet output and provenance (Phase 1). Spike A's run and multi-workspace work feeds into them.
- The native engine (Phase 4).

## Phase 0 is done when

- Every criterion in A1–A12 and B1–B7 is met, or the gap is written up in `RESULTS.md` with a decision about what to do instead.
- D-10 and D-11 are marked accepted or replaced.
- The conformance suite has started: the A3 and A6 outputs are recorded under `corpus/` as the first reference results, keyed by engine version.
