# Phase 0 results

Measurements against the criteria in `docs/phase-0-spec.md`. Each entry says where and when it was measured. Windows results on Thomas's Surface Studio are the ones that count; other platforms are listed for comparison.

## Spike A: bridge

| # | Criterion | Result | Status |
|---|---|---|---|
| A1 | Bridge starts on NetLogo's bundled runtime from a path with spaces | NetLogo 7.0.4 on Java 17.0.2 (Oracle Corporation) from `C:\Program Files\NetLogo 7.0.4\runtime\bin\java.exe` | Met |
| A2 | Start to first reporter result, target under 5 s | 2.91 s: handshake 0.21 s, open AA4-Combined 2.70 s, first `workspace.observe` 0.01 s | Met |
| A3 | AA4 `N3-limit1` metrics match NetLogo's headless BehaviorSpace | Not yet measured | Open |
| A4 | View frames at 30 fps | Not yet measured (needs `workspace.run`) | Open |
| A5 | Agent state at 30 updates/s with 10,000 turtles | Not yet measured | Open |
| A6 | 8 parallel workspaces match single runs | Two workspaces in one bridge checked (`WorkspaceTest.observingDoesNotChangeTheRun`); 8-way test not yet written | Partial |
| A7 | `gis` and `nw` examples run | Not yet measured | Open |
| A8 | Primitive table | Not yet implemented | Open |
| A9 | Legacy `.nlogo` conversion | Not yet implemented | Open |
| A10 | Bridge exits within 2 s when stdin closes | 62 ms | Met |
| A11 | Structured errors, bridge keeps running | Compiler error with span 6–8 for `crt 1 fd`; runtime error for `ask turtle 999999 [ fd 1 ]`; workspace usable afterwards | Met |
| A12 | Observation doesn't change runs | AA4-Combined seed 7, 40 steps, agentset reporters read every step: identical to an unobserved run. With observation on the main RNG the runs diverge (20 vs 17 households at year 840). | Met |

Measured 2026-10-09, Windows, bridge commit on `main` after merge of `phase0/rust-bridge-client`, `cargo test -p logos-bridge-client -- --ignored`.

## Cross-platform determinism

AA4-Combined, `set run-seed 3 setup`, `repeat 60 [ go ]`, run in two separate bridge processes:

| Platform | year | count households | sum [age] of households | newest household (who, xcor, ycor) |
|---|---|---|---|---|
| Windows, Java 17.0.2 (NetLogo bundled) | 860 | 15 | 231 | 654, 23, 71 |
| Linux, Java 21.0.12 | 860 | 15 | 231 | 654, 23, 71 |

Both processes on each platform agreed, and the two platforms agreed with each other.

## Spike B: file format and parser

Not started.
