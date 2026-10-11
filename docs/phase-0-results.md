# Phase 0 results

Measurements against the criteria in `docs/phase-0-spec.md`. Each entry says where and when it was measured. Windows results on Thomas's Surface Studio are the ones that count; other platforms are listed for comparison.

## Spike A: bridge

| # | Criterion | Result | Status |
|---|---|---|---|
| A1 | Bridge starts on NetLogo's bundled runtime from a path with spaces | NetLogo 7.0.4 on Java 17.0.2 (Oracle Corporation) from `C:\Program Files\NetLogo 7.0.4\runtime\bin\java.exe` | Met |
| A2 | Start to first reporter result, target under 5 s | 2.91 s: handshake 0.21 s, open AA4-Combined 2.70 s, first `workspace.observe` 0.01 s. A second run: 2.95 s. | Met |
| A3 | AA4 `N3-limit1` metrics match NetLogo's headless BehaviorSpace | 60 runs, 1,500 metric values (25 metrics, including the time-series lists) and every `[step]` count identical to NetLogo 7.0.4's table output, on Windows (8 workers, 168 s) and Linux. See "A3 detail". | Met |
| A4 | View frames at 30 fps | Viewer spike on AA4 (320 x 480 view), 30 fps cap: 324 frames received and 324 shown, 28.7 fps over 11.3 s. Bridge with a frame every iteration: 41.1 fps. See "A4 detail" below. | Met |
| A5 | Agent state at 30 updates/s with 10,000 turtles | Generated model, 10,000 moving turtles, 518 KiB frames. Windows: 30.2 updates/s at a 30 fps cap; 219 updates/s with a frame after every tick. Linux: 30.1 and 150-168. See "A5 detail". | Met |
| A6 | 8 parallel workspaces match single runs | AA4 seeds 1-8, 150 steps, on 8 workspaces in one bridge: identical to the same runs on one workspace, on Windows and Linux. Windows: 13.2 s on 8 workspaces, 28.4 s on one. A3 also ran on 8 parallel workspaces on Windows. | Met |
| A7 | `gis` and `nw` examples run | `GIS General Examples` (17 procedures: loading shapefiles and rasters, drawing, turtles from points and polygons, coverage, raster sampling, convolution) and `NW General Examples` (generators, centrality, clustering, Louvain, cliques, layouts) run without errors, and two workspaces with the same seed agree on every value read. Every value read is identical on Windows and Linux (see "A7 detail"). Comparison with the NetLogo GUI not done. | Met |
| A8 | Primitive table | `engine.primitives` returns 459 core primitives with argument types, return type, precedence and agent class. Extensions: `nw` 51 and `table` 18 on Windows; `nw` 51 and `gis` 72 on Linux. See "A8 and A9 detail". | Met |
| A9 | Legacy `.nlogo` conversion | Long House Valley (NetLogo 5.0.3) and Ants Benchmark (NetLogo 6.0-M4) converted by `model.convertLegacy` are byte-identical to NetLogo 7.0.4 GUI conversions with "Skip" at the widget-size prompt (D-18), on Windows and Linux. See "A8 and A9 detail". | Met |
| A10 | Bridge exits within 2 s when stdin closes | 62 ms; 42 ms on a second run | Met |
| A11 | Structured errors, bridge keeps running | Compiler error with span 6–8 for `crt 1 fd`; runtime error for `ask turtle 999999 [ fd 1 ]`; workspace usable afterwards | Met |
| A12 | Observation doesn't change runs | AA4-Combined seed 7, 40 steps, agentset reporters read every step: identical to an unobserved run. With observation on the main RNG the runs diverge (20 vs 17 households at year 840). | Met |

Measured 2026-10-09, Windows. A1, A2, A10-A12: `main` after merge of `phase0/rust-bridge-client`, `cargo test -p logos-bridge-client -- --ignored`. A3, A6, A7: branch `phase0/conformance`, `cargo test -p logos-experiment -- --ignored` and the `extensions` test. A5: branch `phase0/agent-state`, the `agents` test. A8, A9: branch `phase0/model-tools`, the `model_tools` test.

## A3 detail

`logos-experiment` reads `N3-limit1` from the model file, lists its 60 runs in BehaviorSpace's order, and checks the run numbers and variable values against the reference table before running anything. Each run sets the variables, runs `setup`, runs `go` with `workspace.run` until the model stops itself, and reads the 25 metrics with `workspace.observe`. Numbers are compared exactly after parsing NetLogo's printed form.

- BehaviorSpace counts 551 steps for a run that the bridge counts as 552 iterations: it does not count the final `go` that executed `stop`. The runner reports steps the same way.
- NetLogo's headless BehaviorSpace runs with plot updates off unless `--update-plots` is given; the bridge runs with them on. For AA4 this makes no difference to any metric. A model whose plot code changes model state or draws random numbers could differ, so the experiment runner should turn plot updates off to match BehaviorSpace (open item for Phase 1).
- Time for 60 runs: 168 s on Windows with 8 workers; 293 s on Linux with 2 workers on 2 cores.

## A4 detail

AA4-Combined, `set run-seed 1 setup`, `workspace.run` of `go` until the model stops itself (552 iterations, year 1351), Windows, NetLogo's bundled Java 17.0.2. From `a4_run_speed_with_and_without_frames` and the view-window spike.

| Setting | Run time | Frames | Frame rate |
|---|---|---|---|
| No frames | 11.12 s (49.6 iterations/s) | - | - |
| 30 fps cap | 9.84 s | 286 | 29.1 fps |
| Frame after every iteration | 13.46 s | 553 | 41.1 fps |
| View-window spike, 30 fps cap | 11.41 s (48.4 iterations/s) | 324 received, 324 shown | 28.7 fps |

- Rendering a frame (NetLogo's `exportView`, conversion to RGBA, writing to the pipe) costs about 3.8 ms, measured from the difference between "no frames" and "every iteration" on Linux and confirmed by these Windows numbers.
- The 30 fps run finishing faster than the run with no frames is most likely run-to-run variation: the "no frames" run is the first in the test, so it probably includes JIT warm-up. The test does not control for this. The comparison that matters is that a 30 fps view adds little to a run, and a frame per iteration adds about 20 percent for this model.
- The viewer showed every frame it received; display is not the bottleneck.
- Visual check (Thomas): the animation was smooth and matched NetLogo's own view of AA4.
- Frames can only be sent between iterations. With AA4's 20 ms iterations, the first version of the rate limiter reached 21 fps against a 30 fps cap; it was changed to schedule frames on fixed due times (commit "Bridge: schedule view frames on fixed due times").

## Cross-platform determinism

AA4-Combined, `set run-seed 3 setup`, `repeat 60 [ go ]`, run in two separate bridge processes:

| Platform | year | count households | sum [age] of households | newest household (who, xcor, ycor) |
|---|---|---|---|---|
| Windows, Java 17.0.2 (NetLogo bundled) | 860 | 15 | 231 | 654, 23, 71 |
| Linux, Java 21.0.12 | 860 | 15 | 231 | 654, 23, 71 |

Both processes on each platform agreed, and the two platforms agreed with each other.

## A5 detail

`cargo test -p logos-bridge-client --test agents -- --ignored`. The model is generated by the test: 10,000 turtles that turn and move every tick and 50 patches that change color every tick, in a 101 x 101 world. Each setting runs `go` for 5 s; frames are counted as they arrive and decode in Rust. The last frame's ticks, `xcor` and `heading` of turtle 4321 equal what `workspace.observe` reports after the run.

| Setting | Windows (Java 17.0.2): updates/s | ticks/s | Linux (2 cores, Java 21.0.12, two runs): updates/s | ticks/s |
|---|---|---|---|---|
| No frames | - | 368 | - | 463, 413 |
| 30 fps cap | 30.2 | 505 | 30.1, 30.1 | 442, 384 |
| Frame after every tick | 219 | 219 | 150, 168 | 149, 168 |

- Frame size: 530,853 bytes (25-byte header, 20 bytes of world bounds, 4 bytes per patch, 49 bytes per turtle).
- Reading, encoding, sending and decoding one frame costs about 1.9 ms on Windows and 3.5-4.5 ms on Linux, from the slowdown when every tick sends a frame.
- The Windows run with the 30 fps cap was faster than the run with no frames. As in A4, the first setting in the test probably includes JIT warm-up; the test does not control for it.
- Bridge tests check the frame contents against the model (positions, headings, sizes, colors as numbers and RGB lists, breeds, hidden turtles, patch order) and that a run with agent frames gives the same final state as a run without them.

## A8 and A9 detail

A8, `cargo test -p logos-bridge-client --test model_tools a8 -- --ignored`. Windows (default `nw,table`): 528 primitives, core 459, nw 51, table 18. Linux with `LOGOS_A8_EXTENSIONS=nw,gis` (the Linux copy of the install has only those two extensions): 582 primitives, core 459, nw 51, gis 72. Types are reported as NetLogo's bit mask plus NetLogo's name for it; for example `fd` takes `number` and runs in turtles only (`-T--`), `of` takes a `reporter block` on the left and is right-associative.

A9, `cargo test -p logos-bridge-client --test model_tools a9 -- --ignored` with the files in `corpus/legacy` (corpus/README.md). Same result on Windows and Linux:

| Source | Version | Compared with the GUI ("Skip") | Warnings |
|---|---|---|---|
| `lhv.nlogo` (Long House Valley) | NetLogo 5.0.3 | byte-identical, 66,992 bytes | none |
| `ants-benchmark.nlogo` | NetLogo 6.0-M4 | byte-identical, 43,833 bytes | none |

- NetLogo's auto-conversion rewrote the 5.0.3 code, for example `foreach potfarms [ ask ? [ ...` became `foreach potfarms [ ?1 -> ask ?1 [ ...`.
- With "Resize and adjust positions" at the widget-size prompt, the GUI's files differ only in the widget elements (positions and sizes, and no `sizeVersion="0"`); code, info, shapes and settings are identical. That re-layout measures Swing components and is left out of the conversion (D-18).
- The source file is unchanged after conversion, and converting to an existing output fails with `1007 FileExists`.

## A7 detail

`cargo test -p logos-bridge-client --test extensions -- --ignored`, `random-seed 42` before `setup`. Values were identical on Windows (Java 17.0.2) and Linux (Java 21.0.12).

| Model | Reporter | Value |
|---|---|---|
| GIS General Examples | `count cities` | 606 |
| | `sum [population] of cities` | 855671431 |
| | `count cities with [ size = 3 ]` | 11 |
| | `count citizens` | 12500 |
| | `count patches with [ pcolor = cyan ]` | 0 |
| | `sum [patch-population] of patches with [ patch-population > 0 ]` | 479107832881.0984 |
| | `count patches with [ (elevation <= 0) or (elevation >= 0) ]` | 10440 |
| NW General Examples | `count turtles` | 100 |
| | `count links` | 960 |
| | `length nw:louvain-communities` | 7 |
| | `[ precision nw:betweenness-centrality 6 ] of turtle 0` | 84.379193 |

## Notes from A3

- `netlogo-headless.bat` in NetLogo 7.0.4 fails when NetLogo is installed in a path with spaces; the reference table was produced by calling Java directly (D-17, corpus/README.md).

## Spike B: file format and parser

Measured 2026-10-11 on Linux (cloud, 2 cores, Java 21.0.12; the NetLogo install there has only the `gis` and `nw` extensions). Corpus: C1, the 575 `.nlogox` files of the NetLogo 7.0.4 Models Library; C2, the 12 `.nlogox` test files of the NetLogo repository at `v7.0.4`; C3, AA4-Combined; and the 4 GUI conversions in `corpus/legacy`. 592 files and 3 `.nls` include files in all. C4 (CoMSES) is not assembled yet. Windows runs pending.

| # | Criterion | Result | Status |
|---|---|---|---|
| B1 | Unmodified round-trip | 592 of 592 files byte-identical; in every file the parsed spans cover every byte of every element's content | Met (Linux; C4 pending) |
| B2 | Edit round-trip | Code replaced with itself plus a comment line in all 592 files, and one slider's `max` changed in the 450 that have a slider: each edit changed only the bytes of its own region, and both read back | Met (Linux; C4 pending) |
| B3 | NetLogo accepts edited files | Every edited file opens in NetLogo 7.0.4 through the bridge with the same result as the original: 522 compile before and after; 70 fail the same way before and after (68 need an extension the Linux install lacks; 2 are NetLogo's own test files without a view, which NetLogo refuses to open) | Met (Linux) |
| B4 | Lexer is lossless | 592 code sections and 3 `.nls` files, 3.88 MB, 761,254 tokens: joining the tokens reproduces every source, and so does the syntax tree's text. The parser reports no errors in any of them | Met (Linux) |
| B5 | Structure matches NetLogo | 514 models that compile here and have no system dynamics diagram: globals, `turtles-own`, `patches-own`, `links-own`, breeds and link breeds (with singular names and `-own` variables) and procedures (name, reporter or command, parameters, including `__includes` files) identical to `workspace.program`, 0 differences. 8 system dynamics models not compared (NetLogo adds code generated from the diagram) | Met (Linux) |
| B6 | Error recovery | Hand-written broken sources (`crates/logos-syntax/tests/parse.rs`): missing `end` before another procedure, before a declaration and at the end of the file; unclosed `[`, stray `]`, `]` closing over an open `(`, unclosed declaration list; unterminated string; code outside procedures. Each is reported at the right span and the rest of the file still parses. Every prefix and suffix of a test model parses without panicking and losslessly | Met |
| B7 | Speed | Largest source, `Continental Divide.nlogox` (101,131 bytes of code): lex and parse in 2.73 ms (release build, Linux). Windows pending | Met (Linux) |

## Spike B detail

`logos-nlogox` keeps the file's text and an element tree with the byte span of every element, attribute and text run (read with `quick-xml`). Writing an unmodified file returns its bytes; an edit replaces one span and re-reads the file. New text is encoded the way NetLogo 7.0.4's `XMLWriter` writes it:

- Element text is written as a CDATA section only when it contains `<`, `>` or `&`, and as plain text otherwise.
- Inside CDATA, NetLogo writes `]]>` as `]]`, U+E000, `>`, and its reader turns that back (`XMLElement.CDataEscape`). `logos-nlogox` does the same in both directions.
- NetLogo's reader skips text that is only whitespace, so a code section containing only whitespace reads as empty.
- Line ends in new text follow the file (CRLF or LF).

`logos-syntax` follows NetLogo's `TokenLexer` and `Charset`:

- Identifiers may contain ``_.?=*!<>:#+/%$^'&-``, so `'` alone is an identifier. The Long House Valley model's `globals [ ... ' booleans ]` declares globals named `'` and `booleans`, and NetLogo agrees (B5).
- A number starts with a digit, or with `.` or `-` followed by a digit or by `.` and a digit, and continues with any identifier characters, so `5-3` is one invalid number token.
- Whitespace is Java's `Character.isWhitespace`, which excludes no-break spaces.

The tree is a `rowan` green tree (D-11). Inside procedure bodies the parser records only bracket and parenthesis groups; expressions are Phase 1 work.

`workspace.program` (PROTOCOL.md) gives NetLogo's own view for B5. NetLogo's `Procedure.args` also lists local variables (`let`) and internal slots such as `_repeatlocal:1`; the bridge reports the declared parameters (`argTokens`).
