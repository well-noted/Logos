# Phase 0 results

Measurements against the criteria in `docs/phase-0-spec.md`. Each entry says where and when it was measured. Windows results on Thomas's Surface Studio are the ones that count; other platforms are listed for comparison.

## Spike A: bridge

| # | Criterion | Result | Status |
|---|---|---|---|
| A1 | Bridge starts on NetLogo's bundled runtime from a path with spaces | NetLogo 7.0.4 on Java 17.0.2 (Oracle Corporation) from `C:\Program Files\NetLogo 7.0.4\runtime\bin\java.exe` | Met |
| A2 | Start to first reporter result, target under 5 s | 2.91 s: handshake 0.21 s, open AA4-Combined 2.70 s, first `workspace.observe` 0.01 s. A second run: 2.95 s. | Met |
| A3 | AA4 `N3-limit1` metrics match NetLogo's headless BehaviorSpace | Not yet measured | Open |
| A4 | View frames at 30 fps | Viewer spike on AA4 (320 x 480 view), 30 fps cap: 324 frames received and 324 shown, 28.7 fps over 11.3 s. Bridge with a frame every iteration: 41.1 fps. See "A4 detail" below. | Met |
| A5 | Agent state at 30 updates/s with 10,000 turtles | Not yet measured | Open |
| A6 | 8 parallel workspaces match single runs | Two workspaces in one bridge checked (`WorkspaceTest.observingDoesNotChangeTheRun`); 8-way test not yet written | Partial |
| A7 | `gis` and `nw` examples run | Not yet measured | Open |
| A8 | Primitive table | Not yet implemented | Open |
| A9 | Legacy `.nlogo` conversion | Not yet implemented | Open |
| A10 | Bridge exits within 2 s when stdin closes | 62 ms; 42 ms on a second run | Met |
| A11 | Structured errors, bridge keeps running | Compiler error with span 6–8 for `crt 1 fd`; runtime error for `ask turtle 999999 [ fd 1 ]`; workspace usable afterwards | Met |
| A12 | Observation doesn't change runs | AA4-Combined seed 7, 40 steps, agentset reporters read every step: identical to an unobserved run. With observation on the main RNG the runs diverge (20 vs 17 households at year 840). | Met |

Measured 2026-10-09, Windows, bridge commit on `main` after merge of `phase0/rust-bridge-client`, `cargo test -p logos-bridge-client -- --ignored`.

## A4 detail

AA4-Combined, `set run-seed 1 setup`, `workspace.run` of `go` until the model stops itself (552 iterations, year 1351), Windows, NetLogo's bundled Java 17.0.2. From `a4_run_speed_with_and_without_frames` and the view-window spike.

| Setting | Run time | Frames | Frame rate |
|---|---|---|---|
| No frames | 11.12 s (49.6 iterations/s) | - | - |
| 30 fps cap | 9.84 s | 286 | 29.1 fps |
| Frame after every iteration | 13.46 s | 553 | 41.1 fps |
| View-window spike, 30 fps cap | 11.41 s (48.4 iterations/s) | 324 received, 324 shown | 28.7 fps |

- Rendering a frame (NetLogo's `exportView`, conversion to RGBA, writing to the pipe) costs about 3.8 ms, measured from the difference between "no frames" and "every iteration" on Linux and confirmed by these Windows numbers.
- The 30 fps run finishing faster than the run with no frames is run-to-run variation (the "no frames" run is the first in the test and includes JIT warm-up). The test does not control for it. The comparison that matters is that a 30 fps view adds little to a run, and a frame per iteration adds about 20 percent for this model.
- The viewer showed every frame it received; display is not the bottleneck.
- Frames can only be sent between iterations. With AA4's 20 ms iterations, the first version of the rate limiter reached 21 fps against a 30 fps cap; it was changed to schedule frames on fixed due times (commit "Bridge: schedule view frames on fixed due times").

## Cross-platform determinism

AA4-Combined, `set run-seed 3 setup`, `repeat 60 [ go ]`, run in two separate bridge processes:

| Platform | year | count households | sum [age] of households | newest household (who, xcor, ycor) |
|---|---|---|---|---|
| Windows, Java 17.0.2 (NetLogo bundled) | 860 | 15 | 231 | 654, 23, 71 |
| Linux, Java 21.0.12 | 860 | 15 | 231 | 654, 23, 71 |

Both processes on each platform agreed, and the two platforms agreed with each other.

## Spike B: file format and parser

Not started.
