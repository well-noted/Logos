# Phase 1 results

Measurements against the criteria in `docs/phase-1-spec.md`, part by part, as the steps land.

## Part P: projects

| # | Criterion | Result | Status |
|---|---|---|---|
| P1 | `logos project init` on AA4-Combined and MASTOC-LLM | AA4: the five `data/*.txt` inputs from `file-open`; `data/log.txt` (a change log the model never reads) offered for review, not added. MASTOC-LLM: the five `Data/*.txt` files as outputs (`file-open` followed by `file-print`), `mastoc_llm_bridge.py` from `py:run "import ..."`, `requirements.txt`, `behaviourspace.xml`, `.env.example`, and the key names read by the Python code as optional secrets; the Python bridge's own `logs/<run>/` output is built at run time and can't be detected. Reviewed by Thomas with `--dry-run` on Windows, 2026-10-11 | Met |
| P2 | A project folder opens unchanged in NetLogo 7.0.4 | `init` writes only `logos.toml`; the model and data files are not touched. GUI check in NetLogo not yet done | Open |
| P3 | `logos check` | Reports a missing input file, an output whose folder doesn't exist, invalid data dictionaries, a missing Python 3 interpreter, uninstalled packages from `requirements.txt`, unset required secrets, and (with `--compile`) NetLogo compile errors with line numbers. AA4 clean on Windows and Linux, including the NetLogo compile | Met |

## Part K: secrets and environment

Measured 2026-10-11 on Linux (cloud, no Secret Service, so the encrypted file store) and on Windows (Credential Manager, by Thomas).

| # | Criterion | Result | Status |
|---|---|---|---|
| K1 | Set, list (masked), unmask, change and delete from the CLI | `logos secrets set/list/show/delete` work with the encrypted file store. `show` refuses without an interactive terminal and asks for the name to be typed before printing; every set, import, show and delete is logged by name and time in `secrets-access.log`. On Windows the value is stored in Credential Manager (entry `user/LOGOS_TEST_KEY` under `logos`), `show` prints it after the name is typed, and `delete` removes the entry. macOS Keychain not yet run | Met on Linux (file store) and Windows (Credential Manager); macOS pending |
| K2 | No secret in any file Logos writes | With fake keys set and imported: no value found in `logos.toml`, the configuration folder (`secrets.enc` is encrypted, names included; `secrets-access.log` has names only) or the scrubbed `.env`. The bridge receives values as environment variables, and its configuration prints `[hidden]` for them. The full check (a recorded MASTOC-LLM run) needs the experiment runner and Part L | Partly met |
| K3 | `.env` import | `logos secrets import-env` stores each value, declares new names as optional in `logos.toml`, and with the user's agreement rewrites the file with values removed (names, comments and line endings kept) | Met |
| K4 | No credential store | On Linux without a Secret Service, the passphrase-encrypted file is used (Argon2id key, XChaCha20-Poly1305, a new nonce on every write); a wrong passphrase is rejected; nothing is written in plain text | Met |

## Part E: experiments and provenance

Measured 2026-10-11 on Linux (cloud, 2 processors, OpenJDK 21) and on Windows (Thomas's Surface Laptop Studio: Intel Core i7-11370H, 4 cores, 8 logical processors, 16 GB RAM; NetLogo's bundled Java).

The bridge reads experiments with NetLogo's own loader and runs each run with NetLogo's own BehaviorSpace code (`org.nlogo.lab.Worker.Runner`), so settings, stop conditions, metric timing, sub-experiments and error handling are BehaviorSpace's by construction. Logos schedules the runs and writes the output (PROTOCOL.md section 5.4).

| # | Criterion | Result | Status |
|---|---|---|---|
| E1 | Results match NetLogo | NetLogo's own BehaviorSpace test cases (`test/lab` at v7.0.4): all 36 table outputs identical on Linux and Windows, covering metrics every step, stop conditions, time limits, setting `random-seed` and world size, sub-experiments, runtime errors in setup, go and metrics, and experiments stored in a model. Two files in that folder (`testStats`, `testStatsExcludeMetrics`) are not table references: NetLogo's test uses them only as input to its statistics output, and NetLogo 7.0.4's headless BehaviorSpace doesn't reproduce them either. AA4: all eight experiments (1,720 runs) identical, row for row, to tables written by NetLogo's headless BehaviorSpace on Windows | Met |
| E2 | Parquet output | `results.parquet`, one row per measurement, typed columns. Every value read back equals the table output, for the 36 NetLogo test cases and all eight AA4 experiments | Met |
| E3 | Provenance | Every saved run file holds a run record: start and end time, workspace, the `random-seed` setting when there is one, Logos, NetLogo, bridge and Java versions, operating system, the names of secrets passed to the engine, and the SHA-256 of the model, `logos.toml`, the experiments file and every project file the model reads or runs, with data dictionaries. `--resume` refuses when the engine or any of those files changed. AA4 `N3-limit1` on Windows: all 60 runs have a complete record with one shared provenance (model and five data files hashed), and the table matches NetLogo's reference. After a run was stopped and one data file changed, the file's hash differed and `--resume` refused (exit code 1) | Met |
| E4 | Resume | AA4 `N3-limit1` on Windows: the process was killed after 3 runs had finished; `logos run --resume` ran the other 57, and the table is identical to NetLogo's reference table. No bridge process was left running. Ctrl-C (stop after the current step, not saved) checked on Linux | Met |
| E5 | Throughput | `N3-limit1`, 8 workers against NetLogo with 8 threads, on Thomas's machine. Release build: 165.1, 170.9 and 167.5 s; NetLogo: 211.2, 180.1 and 179.1 s. A debug build is slower than NetLogo (median 207.0 s), so timings are taken with release builds | Met |

Notes from the Windows runs:

- `netlogo-headless.bat` in `C:\Program Files\NetLogo 7.0.4` fails because of the space in the path (D-17). Reference tables were recorded by starting NetLogo's bundled `java.exe` directly.
- The seven new AA4 reference tables (19 MB) are in `corpus/reference` on Thomas's machine and not committed.
- One bridge failed to start with Windows error 1455 (page file too small) while a NetLogo experiment with 8 threads was running at the same time. Not seen otherwise.

## Part L: models that call language models

Moved to Phase 3, where Logos has a user interface (D-20). Not measured in Phase 1.

## Part S: language support

Measured 2026-10-11 on Linux (cloud). The NetLogo copy there has only the `gis` and `nw` extensions, so models using other extensions have no primitive table and are not parsed; the Windows run covers them.

The expression parser (`logos_syntax::parse_with`) takes the primitive table from NetLogo through the bridge: `engine.primitives` now also returns the built-in agent variables, constants, keywords, the primitives NetLogo derives from breeds (found by asking NetLogo's own breed handling), and the type masks the parser needs (D-01). It records what every name refers to (primitive, breed primitive, procedure, global, agent variable, local) and where each local is defined.

| # | Criterion | Result | Status |
|---|---|---|---|
| S1 | Parser agrees with NetLogo on references | Step 14 | Open |
| S2 | No false errors | 576 corpus models: 508 parsed, none of them reports an error while compiling in NetLogo; 68 skipped (extensions not installed here). Models with a system dynamics diagram get the globals and procedures NetLogo generates from it through `workspace.program` | Partly met |
| S3 | Rename is safe | Step 17 | Open |
| S4 | Diagnostics positions | Step 15 | Open |
| S5 | Responsiveness | Step 16 | Open |
| S6 | Lossless | The full tree's text equals the source for all 508 parsed models | Partly met |
