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
