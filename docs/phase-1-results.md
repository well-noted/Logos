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

Measured 2026-10-11 on Linux (cloud, 2 processors, OpenJDK 21).

The bridge reads experiments with NetLogo's own loader and runs each run with NetLogo's own BehaviorSpace code (`org.nlogo.lab.Worker.Runner`), so settings, stop conditions, metric timing, sub-experiments and error handling are BehaviorSpace's by construction. Logos schedules the runs and writes the output (PROTOCOL.md section 5.4).

| # | Criterion | Result | Status |
|---|---|---|---|
| E1 | Results match NetLogo | NetLogo's own BehaviorSpace test cases (`test/lab` at v7.0.4): all 36 table outputs identical, covering metrics every step, stop conditions, time limits, setting `random-seed` and world size, sub-experiments, runtime errors in setup, go and metrics, and experiments stored in a model. Two files in that folder (`testStats`, `testStatsExcludeMetrics`) are not table references: NetLogo's test uses them only as input to its statistics output, and NetLogo 7.0.4's headless BehaviorSpace doesn't reproduce them either. AA4 `N3-limit1`: all 60 rows identical to the Windows reference table, as text. AA4's other seven experiments: reference tables being recorded with NetLogo's headless BehaviorSpace (step 6) | Partly met |
| E2 | Parquet output | `results.parquet`, one row per measurement, typed columns. For all 36 NetLogo test cases every value read back from Parquet equals the table output. AA4 not yet checked | Partly met |
| E3 | Provenance | Step 8 | Open |
| E4 | Resume | `logos run --resume DIR` runs only the runs the folder doesn't have; Ctrl-C stops runs in progress after their current step and they are not saved. Checked here on a 6-run experiment interrupted after 3 runs. The comparison with an uninterrupted run on AA4 is in the test handoff | Partly met |
| E5 | Throughput | Step 8, on Thomas's machine. For reference, `N3-limit1` took 277 s here with 2 workers | Open |
