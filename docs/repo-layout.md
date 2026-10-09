# Repository layout and Windows build

This is a proposal. Nothing here is built yet. The layout is designed so that an AI coding agent can work on one crate at a time, run that crate's tests alone, and open a PR with one purpose.

## Layout

```
logos/
├── Cargo.toml                 Rust workspace root
├── rust-toolchain.toml        selects the stable channel with rustfmt and clippy
├── LICENSE-MIT
├── LICENSE-APACHE
├── README.md
├── AGENTS.md                  instructions for AI agents: build, test, PR rules
├── crates/
│   ├── logos-syntax/          lexer and lossless syntax tree for NetLogo code
│   ├── logos-nlogox/          lossless .nlogox reader and writer
│   ├── logos-project/         project model: model file, __includes, settings, engine pin
│   ├── logos-semantic/        symbols, scopes, references (uses primitive tables from the bridge)
│   ├── logos-protocol/        bridge message types (Rust side), generated or hand-kept in sync with protocol/
│   ├── logos-bridge-client/   starts, supervises and talks to the bridge process
│   ├── logos-conformance/     corpus manifest, runner, comparison of recorded outputs
│   ├── logos-cli/             the `logos` binary
│   └── xtask/                 build automation written in Rust (cargo xtask ...)
├── spikes/
│   └── view-window/           Phase 0 viewer; throwaway, not depended on by any crate
├── bridge/                    JVM bridge, GPL-3.0-or-later (see docs/decisions.md D-01)
│   ├── LICENSE                GPL-3.0 text
│   ├── settings.gradle.kts
│   ├── build.gradle.kts
│   ├── gradlew, gradlew.bat   Gradle wrapper (committed)
│   └── src/
│       ├── main/java/org/logos/bridge/
│       └── test/java/org/logos/bridge/
├── protocol/
│   └── PROTOCOL.md            message definitions; the single source of truth for both sides
├── corpus/
│   ├── manifest.toml          model list: source, URL or install path, SHA-256, license, tags (gis, nw, ...)
│   ├── own/                   Thomas's models, committed if he chooses
│   │   └── aa4-combined/      primary test model and its data/ files (D-15)
│   └── .cache/                downloaded models (git-ignored)
├── docs/
│   ├── decisions.md
│   ├── repo-layout.md
│   └── phase-0-spec.md
└── .github/workflows/
    └── ci.yml                 windows-latest first, then ubuntu-latest and macos-latest
```

### Why these boundaries

- `logos-syntax` and `logos-nlogox` have no dependency on the bridge. They can be tested with plain `cargo test` and no Java installed. Most early agent work lands here.
- `logos-semantic` depends on primitive tables, which come from the bridge. Its tests use a primitive-table JSON fixture produced once from NetLogo 7.0.4 and stored under `crates/logos-semantic/tests/fixtures/`. The fixture is generated data about NetLogo's public language (names and syntax), not NetLogo source. Confirm this during the license review.
- `logos-protocol` keeps message types in one place, so a protocol change is one PR touching `protocol/`, `logos-protocol` and the bridge.
- `spikes/` holds Phase 0 code that is allowed to be rough. Nothing outside `spikes/` may depend on it.
- `xtask` replaces shell scripts. It runs the same way in PowerShell, cmd and bash, which avoids maintaining `.ps1` and `.sh` versions of every script.

## Toolchain

| Tool | Version | Used for |
|---|---|---|
| Rust (rustup) | stable; minimum 1.97 (`rust-version` in `Cargo.toml`) | everything under `crates/` |
| Visual Studio Build Tools | 2022, "Desktop development with C++" workload | the MSVC linker Rust needs on Windows |
| JDK | 17 (Eclipse Temurin) | building the bridge with `--release 17`; NetLogo 7.0.4 ships a Java 17.0.2 runtime (D-05) |
| Gradle | through the committed wrapper | building the bridge; no global install |
| NetLogo | 7.0.4, installed normally | the bridge compiles against and runs with its jars and bundled Java runtime |
| Python | 3.11+ (later phases) | Python client, analysis |
| Git | Git for Windows | with `core.longpaths=true` |

Building against the local NetLogo install, not a Maven artifact, keeps the bridge on exactly the engine version that runs the models. NetLogo also publishes artifacts to a Cloudsmith Maven repository (`maven.cloudsmith.io/netlogo/netlogo`), which CI can use if CI machines can't install NetLogo. Check which artifacts and versions are available there before relying on it.

## Finding NetLogo

The bridge needs the NetLogo jar, its `lib` directory, the extensions directory and, at run time, NetLogo's bundled Java runtime. Logos finds them in this order:

1. `LOGOS_NETLOGO_HOME` environment variable.
2. The engine path saved in the project or user settings.
3. Default install locations: on Windows, `C:\Program Files\NetLogo 7.0.4\`; on macOS, `/Applications/NetLogo 7.0.4/`; on Linux, `~/NetLogo 7.0.4/` and `/opt/`.

Layout of the Windows 7.0.4 install, checked on Thomas's machine (`C:\Program Files\NetLogo 7.0.4\`):

| Path | Contents |
|---|---|
| `app\netlogo-7.0.4.jar` | NetLogo. Its manifest `Class-Path` lists the other jars in `app\`, so the classpath only needs this jar plus the bridge jar. |
| `app\*.jar` | Dependencies (Scala 3.7.0 library, parboiled, pekko, JavaFX 21, and others). |
| `runtime\bin\java.exe` | Bundled Java runtime, 17.0.2. |
| `extensions\.bundled\` | Bundled extensions, including `gis`, `nw`, `table`, `csv`, `py`, `time`. User-installed extensions go in `extensions\`. |
| `models\` | Models Library. |
| `natives\windows-amd64\` | Native libraries (passed as `java.library.path`). |
| `netlogo-headless.bat` | NetLogo's own headless launcher; useful as the reference for JVM options and for running BehaviorSpace experiments to compare against. |

JVM options the bridge passes, taken from `app\NetLogo_Console.cfg` and `netlogo-headless.bat`:

```
-XX:MaxRAMPercentage=50
-Dfile.encoding=UTF-8
-Djava.awt.headless=true
-Dnetlogo.extensions.dir=<install>\extensions
-Dnetlogo.models.dir=<install>\models
-Dnetlogo.docs.dir=<install>
-Djava.library.path=<install>\natives\windows-amd64
--add-exports=java.base/java.lang=ALL-UNNAMED
--add-exports=java.desktop/sun.awt=ALL-UNNAMED
--add-exports=java.desktop/sun.java2d=ALL-UNNAMED
```

`cargo xtask find-netlogo` checks for each of these paths and prints what is missing.

### NetLogo source

Thomas's clone at `C:\Users\tetuo\NetLogo` is his fork (`origin` = `well-noted/NetLogo`, `upstream` = `NetLogo/NetLogo`), currently on the `windows-native-file-dialog` branch with uncommitted changes. It doesn't have release tags yet. To read the source that matches the reference engine without disturbing that branch:

```
git fetch upstream tag v7.0.4 --no-tags
git worktree add ..\NetLogo-7.0.4 v7.0.4
```

Logos never builds against this clone. It is for reading only (D-01).

Paths with spaces (`Program Files`) must work. Every place that builds a command line passes arguments as a list, never as one joined string.

## Commands

```
cargo xtask doctor            check that Rust, MSVC, JDK 17 and NetLogo are present; print versions
cargo xtask find-netlogo      print the detected NetLogo install and its layout
cargo xtask build             build the Rust workspace and the bridge
cargo xtask test              Rust tests and bridge tests that don't need NetLogo
cargo xtask test --engine     also run tests that start the bridge against installed NetLogo
cargo xtask corpus fetch      download corpus models listed in corpus/manifest.toml into corpus/.cache
cargo xtask corpus roundtrip  run the .nlogox round-trip check over the corpus
cargo xtask spike-view <model.nlogox>   run the Phase 0 viewer
```

Plain `cargo build` and `cargo test` keep working for Rust-only changes.

## First-time setup on Windows

1. Install Rust with rustup and accept the MSVC toolchain. Install Visual Studio Build Tools 2022 with the C++ workload if rustup asks for it.
2. Install Eclipse Temurin JDK 17 and make sure `JAVA_HOME` points to it. (NetLogo's bundled runtime can run the bridge but has no compiler.)
3. Confirm NetLogo 7.0.4 is installed.
4. `git config --global core.longpaths true`
5. Clone the repository, then run `cargo xtask doctor` from the repository root.

## CI

- Windows is the first job and the one that blocks merges.
- Ubuntu and macOS jobs run the same commands. They start as non-blocking until they pass reliably.
- Jobs without NetLogo run `cargo xtask test`. Engine tests (`--engine`) run on a job that installs NetLogo 7.0.4 or pulls it from Cloudsmith, depending on what Phase 0 finds out.

## Agent and PR conventions (for AGENTS.md)

- One purpose per PR. A PR that changes the protocol touches `protocol/`, `logos-protocol` and `bridge/` together, and nothing else.
- Every PR adds or updates tests. Parser and file-format changes include a corpus case or a minimal fixture that shows the change.
- `cargo fmt`, `cargo clippy -D warnings` and the bridge's formatter pass before a PR is opened.
- No NetLogo source code is copied into anything outside `bridge/` (D-01).
- Code comments and docs use American English.
