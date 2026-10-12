# Phase 1: specification

Phase 1 turns the Phase 0 spikes into a core that a researcher can use from the command line and from any editor:

- **Projects:** a model and the files around it (data, Python modules, settings) as one unit, described in a project file that NetLogo ignores (D-19).
- **Experiments:** BehaviorSpace experiments run on parallel workspaces, with results in Parquet and a provenance record for every run.
- **Models that call language models:** moved to Phase 3 (D-20). Part L below is kept as the specification to start from.
- **Language support:** NetLogo code gets a full expression parser, NetLogo's own compile errors, and editor features through a language server.

Engine: NetLogo 7.0.4 (D-05). Test models: AA4-Combined (D-15) and MASTOC-LLM (Thomas's model; `extensions [ py ]`, a Python bridge module, LLM calls to Anthropic, OpenAI-compatible endpoints and Ollama).

Users keep writing NetLogo. Logos edits NetLogo code and runs it on NetLogo itself through the bridge, so every primitive, extension and file NetLogo accepts works in Logos, and the model file stays a NetLogo model (D-19).

The work is split into five parts. Parts P (project), E (experiments) and K (secrets) come first because the others build on them; parts L (LLM recording) and S (language support) can then proceed in parallel. Each part lists its success criteria and its PRs; every PR has one purpose (AGENTS.md).

---

## Part P: projects

### Scope

A Logos project is a folder that holds one `.nlogox` model and the files it uses. Logos describes the folder in `logos.toml`, next to the model. The model file is never changed to make it part of a project, and a project folder still opens in NetLogo as it is.

### `logos.toml` (first version)

```toml
[project]
model = "AA4-Combined.nlogox"
engine = "netlogo-7.0.4"            # the engine this project was last run on (D-05)

[[files]]                           # every file the model reads or writes
path = "data/water.txt"
role = "input"                      # input | output | code | config
description = "Water sources by period"
dictionary = "data/water.dictionary.toml"   # optional, see below

[[files]]
path = "mastoc_llm_bridge.py"
role = "code"
language = "python"

[python]                            # for models that use the py extension
requirements = "requirements.txt"
interpreter = ""                    # empty: let Logos find one; or a path

[secrets]                           # names only; values never appear in project files
required = ["ANTHROPIC_API_KEY"]
optional = ["OPENAI_API_KEY"]
```

- A data dictionary describes the columns of a data file: name, type, unit, meaning and source. Its format is a small TOML file. Logos does not require one.
- `logos project init` writes a first `logos.toml` by reading the model: files named in `file-open`, `import-*`, `__includes`, `py:run "import ..."` and BehaviorSpace output settings, and files found in the folder. The user confirms the list. Detection from code is a hint; the file list is what the user confirms.
- Unknown keys in `logos.toml` are kept when Logos rewrites it, as for `.nlogox` (Spike B).

### Success criteria

| # | Criterion | Target |
|---|---|---|
| P1 | `logos project init` on AA4-Combined and MASTOC-LLM | Lists every input file, Python module and config file the model uses, and nothing it doesn't, after Thomas's review |
| P2 | A project folder opens unchanged in NetLogo 7.0.4 | AA4 and MASTOC-LLM run in the NetLogo GUI from a folder Logos has initialized |
| P3 | `logos check` | Reports missing input files, a missing Python interpreter or package, and missing required secrets before a run starts, each with the file or name involved |

---

## Part K: secrets and environment

### Scope

Models like MASTOC-LLM need API keys and other environment variables. Logos lets the user set them from inside Logos (Thomas, 2026-10-11) under these rules:

- Values are stored in the operating system's credential store, keyed by project and name: Windows Credential Manager, the macOS Keychain, and the Secret Service API on Linux (GNOME Keyring, KWallet). One cross-platform layer covers all three (for example the `keyring` crate; confirmed in step 4). They are never written to `logos.toml`, `.nlogox`, run records, logs or crash reports.
- Where no credential store is available (a Linux server without a desktop session, a container), Logos does not fall back to a plain file. The user either passes the variables in the environment that starts Logos, or uses a file encrypted with a passphrase entered once per session.
- A project can import an existing `.env` file once; Logos then offers to remove the values from the file. It never writes a `.env` file.
- Values reach the model only as environment variables of the bridge process, and only the names the project declares. The bridge passes them to NetLogo's `py` extension the way it inherits any environment variable.
- Logos shows a stored value masked (`sk-…3f2a`). Changing or deleting a value asks for confirmation.
- The user can unmask a value on request:
  - From the CLI, `logos secrets show NAME` prints it only after an interactive confirmation, and refuses when it isn't run in a terminal, so scripts and agents can't use it.
  - In the desktop app (Phase 3), unmasking first asks the operating system to confirm the user where it offers that (Windows Hello, Touch ID or the account password on macOS, the keyring's unlock prompt on Linux), and otherwise asks for confirmation in Logos. The value is hidden again after 30 seconds, and a copied value is cleared from the clipboard after 30 seconds.
  - Each unmask is logged with the name and time, never the value.
  - Unmasking is never offered over the API, the MCP server or to the in-app assistant.
- Provenance records which secret names a run used, never values. Recorded LLM requests have authentication headers removed before they are written.

### Success criteria

| # | Criterion | Target |
|---|---|---|
| K1 | Set, list (masked), unmask, change and delete a secret from the CLI | Works on Windows, macOS and Linux with a desktop session; unmask refuses without a terminal |
| K2 | No secret in any file Logos writes | A test runs MASTOC-LLM with a known fake key and searches every file Logos wrote (project, run records, logs) for it: no match |
| K3 | `.env` import | Values move into the credential store; the user is offered removal of the file's values |
| K4 | No credential store | On Linux without a Secret Service, Logos uses the passphrase-encrypted file or environment variables and never writes a value in plain text |

---

## Part E: experiments and provenance

### Scope

`logos-experiment` (Phase 0) becomes a complete BehaviorSpace runner. It reads experiments from the model file or a separate BehaviorSpace XML file, runs them on parallel workspaces in one bridge (D-09), and writes:

- **Parquet:** one table per experiment with one row per measurement (run number, step, every varied parameter, every metric), with column types from the values (numbers as doubles, lists as Parquet lists, strings, booleans). Uses the Apache `arrow` and `parquet` crates.
- **NetLogo's own formats:** table and spreadsheet CSV identical to what headless BehaviorSpace writes, so existing analysis scripts keep working.
- **A run record per run:** a small JSON file per run (or rows in a run table) with: engine and bridge versions, operating system, model file hash, project file hash, hash of every input file, the run's random seed when the model sets one, parameter values, start and end time, step count, how the run ended, and the names of secrets used.

BehaviorSpace features to support (all of 7.0.4): metrics every step or at the end, `runMetricsCondition`, stop conditions, pre-run and post-run commands, sub-experiments, repetitions in sequential and interleaved order, time limits. Plot updates are off by default, as in headless BehaviorSpace, with an option to turn them on (open item from A3).

An interrupted experiment can be resumed: runs whose records are complete are skipped.

### Success criteria

| # | Criterion | Target |
|---|---|---|
| E1 | Results match NetLogo | All eight BehaviorSpace experiments in AA4-Combined, and every experiment in Models Library models that run headless, give table output identical to NetLogo 7.0.4's headless BehaviorSpace (reference tables recorded under `corpus/reference`, as for A3) |
| E2 | Parquet output | Reading the Parquet file back gives the same values as the table output, for every E1 experiment |
| E3 | Provenance | Every run in E1 has a complete run record; changing an input data file changes the recorded hash |
| E4 | Resume | Killing the runner partway through an experiment and starting it again completes the experiment with results identical to an uninterrupted run |
| E5 | Throughput | AA4 `N3-limit1` (60 runs) on Thomas's machine with 8 workers takes no longer than NetLogo's headless BehaviorSpace with 8 threads (A3 took 168 s through Logos) |

---

## Part L: models that call language models

**Moved to Phase 3 (D-20).** Recording and replaying a model's LLM calls needs provider accounts and keys set up for testing, which is easier once Logos has a user interface. This part stays here as the specification to start from; its criteria are not part of Phase 1.

### Scope

During a run, a model may call a language model, as MASTOC-LLM does from Python through the `py` extension. Those calls are slow, cost money and give different answers each time, so seeded reproducibility (A3, A12) no longer holds for these models. Part L makes such runs recordable, replayable and bounded.

- **Recording:** Logos starts a local recording proxy and points the model's LLM clients at it by setting the base-URL environment variables of the bridge process for each provider. The model's code does not change. Each call is stored with the run: request (authentication removed), response, provider, model name and settings, token counts, time, and the run and step it happened in.
- **Reasoning:** when a response includes the model's reasoning ("thinking" or chain-of-thought content), it is recorded with the call. Some providers return only a summary of the reasoning, return it encrypted or redacted, or don't return it at all; the record states which of these applies, so an analysis never mistakes a missing reasoning trace for an empty one.
- **Replay:** a recorded run can be run again with the proxy answering every call from the recording instead of the provider. With the same seed, the run must reproduce the recorded results exactly. A call that doesn't match the recording (different request at the same position) stops the replay with an error that shows both requests.
- **Budgets:** an experiment can set limits on tokens, on estimated cost and on calls per minute per provider. Logos stops starting new runs when a limit would be exceeded and records why. Cost estimates use a price table the user can edit; Logos does not claim exact billing.
- **Caching (optional per experiment):** identical requests within an experiment can be answered from earlier responses, for development runs. Off by default, and always recorded in the run record when on.

### Open questions for an early spike (L0)

- Which base-URL environment variables each client library used by MASTOC-LLM reads (Anthropic, OpenAI, OpenAI-compatible Ollama), and whether all its calls go through them. Verify against the libraries' documentation and by experiment before relying on it.
- Streaming responses: record the assembled response and the stream boundaries, or only the result.
- Python-side randomness: models that use Python's `random` in the `py` extension need it seeded for replay to be exact. Decide whether Logos seeds it or only reports it.

### Success criteria

| # | Criterion | Target |
|---|---|---|
| L1 | Recording without code changes | A MASTOC-LLM run with each of Anthropic, an OpenAI-compatible endpoint and Ollama records every LLM call the model makes (count checked against the model's own log) |
| L2 | Replay is exact | Replaying a recorded MASTOC-LLM run gives results identical to the recorded run, with no network access |
| L3 | Reasoning recorded or accounted for | For each provider in L1, reasoning content is stored when returned, and the record says when it was summarized, redacted or not provided |
| L4 | Budgets | An experiment with a token limit stops starting runs at the limit; the run records show which runs were skipped and why |
| L5 | No secrets in recordings | K2 holds for all L1 recordings |

---

## Part S: language support

### Scope

- **Expression parser:** extends `logos-syntax` (D-11) from top-level structure to full expressions inside procedure bodies, using the primitive table from `engine.primitives` (A8) for arity, precedence and infix operators, plus the model's own procedures and breeds (breed primitives such as `create-<breeds>` are generated from the declarations). Variadic primitives in parentheses, anonymous procedures (`[ x -> ... ]`), `let`, `set`, `ask` blocks and agentset reporters are all represented in the tree. The tree stays lossless.
- **Diagnostics:** compile errors come from NetLogo through the bridge (D-07), mapped to source positions in the editor's text. Logos's own parser reports only syntax errors it is sure NetLogo also reports, checked against the corpus.
- **Language server (LSP):** `logos lsp` serves any editor that speaks the Language Server Protocol (VS Code first). Features: diagnostics, document outline, go to definition, find references, rename (procedures, globals, breed variables, local variables), hover with the primitive's syntax, completion of primitives and model names, and word-wise selection that respects hyphenated names (`segments`, Spike B).
- **Includes:** `.nls` files from `__includes` are parsed with the model, so definitions and references work across files.

### Success criteria

| # | Criterion | Target |
|---|---|---|
| S1 | Expression parser agrees with NetLogo | For every corpus model that compiles, the parser finds every procedure call and variable reference that NetLogo's compiler resolves, checked against a bridge method that reports NetLogo's resolved references |
| S2 | No false errors | The parser reports no syntax errors on any corpus model that compiles in NetLogo |
| S3 | Rename is safe | Renaming each procedure and each global in AA4 and a sample of 50 corpus models gives a model that compiles in NetLogo and whose B5 structure matches the original with only the renamed name changed |
| S4 | Diagnostics positions | For a set of hand-written compile errors (unknown name, wrong argument count, type mismatch, missing bracket), the reported span matches NetLogo's |
| S5 | Responsiveness | Diagnostics within 500 ms of the last keystroke for AA4 on Thomas's machine; outline and go-to-definition within 50 ms |
| S6 | Lossless | B4 holds for the full tree over the corpus |

---

## The `logos` command

All of Phase 1 is reachable from one command-line program (`crates/logos-cli`):

```
logos project init [folder]       write logos.toml after review
logos check [folder]              files, Python, secrets, engine
logos secrets set|list|delete     credential store (Part K)
logos run <experiment> [--workers N] [--resume] [--record-llm] [--budget ...]
logos replay <run-record>         Part L
logos convert <file.nlogo>        model.convertLegacy (A9)
logos lsp                         language server (Part S)
```

---

## PR breakdown

1. `logos-project`: read and write `logos.toml`, keeping unknown keys. P (format).
2. `logos project init`: detect files from model code and folder. P1.
3. `logos check`. P3.
4. Secrets: credential store, CLI, bridge environment. K1, K3.
5. Experiment runner: complete BehaviorSpace semantics (metrics every step, conditions, pre/post commands, sub-experiments, plots off). E1 on AA4.
6. Reference tables for E1: record NetLogo's headless output for AA4's eight experiments and the library experiments. E1.
7. Parquet writer. E2.
8. Run records and resume. E3, E4, E5.
9. (Moved to Phase 3, D-20.) L0 spike: client base URLs, streaming, Python randomness (findings written to `docs/phase-1-results.md`).
10. (Moved to Phase 3.) Recording proxy and LLM call store. L1, L3, L5, K2.
11. (Moved to Phase 3.) Replay. L2.
12. (Moved to Phase 3.) Budgets and caching. L4.
13. Expression parser. S2, S6.
14. Bridge method for NetLogo's resolved references; S1 check.
15. Diagnostics through the bridge. S4.
16. Language server: outline, definition, references, hover, completion. S5.
17. Rename. S3.
18. `logos` CLI wrapping the above.

## Out of scope for Phase 1

- The interface editor and its widget layout command (Phase 2, D-18).
- The desktop app and the live view in a window (Phase 3; D-10 is decided).
- The Python client, the local API service and the MCP server (Phase 2; D-03).
- Jupyter notebooks (D-16).
- Experiment designs beyond BehaviorSpace's (Latin hypercube, Sobol, Morris; D-08) and sensitivity analysis.
- The in-app assistant (D-04) and connecting coding assistants through the MCP server (Phase 2-3). Part L records the model's own LLM calls; it is not the assistant.
- Logos's own LLM bridge for models (Phase 2). Part L records and replays calls made by any bridge, including MASTOC-LLM's and, later, Logos's.
- 3D models.
- Recording, replay and budgets for models that call language models (Part L; Phase 3, D-20).

## Phase 1 is done when

- Every criterion above is met on Windows, or the gap is written up in `docs/phase-1-results.md` with a decision about what to do instead.
- Thomas has run one of his own experiments (AA4 or MASTOC-LLM) end to end through `logos run` and used the results in his own analysis.
