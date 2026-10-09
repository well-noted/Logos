# Instructions for AI coding agents

Read `docs/decisions.md` before changing anything. Decisions are referred to by number (D-01, D-14, ...).

## Rules

- One purpose per PR. Don't mix refactoring with features.
- Every PR adds or updates tests.
- Never copy or translate NetLogo source code into anything outside `bridge/` (D-01). Reading it to understand behavior is fine.
- Never read model values with `HeadlessWorkspace.report` during a run. Use the bridge's observation path, which runs on a clone of the random number generator (D-14).
- Never modify files under `corpus/` except to add new models. Their bytes are test fixtures.
- Windows is the primary platform. Paths may contain spaces. Pass process arguments as lists, never as one joined string.
- Use American English in code, comments and docs.
- Write plainly. No marketing language in docs or comments.

## Build and test

The commands are listed in `docs/repo-layout.md` under "Commands". Until `xtask` exists, use `cargo build` and `cargo test`.

## Reference engine

NetLogo 7.0.4, installed at `C:\Program Files\NetLogo 7.0.4\` on the main development machine. Read NetLogo source at the `v7.0.4` tag, not `main` (D-05).
