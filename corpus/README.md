# Corpus

Models used by the round-trip tests (Spike B) and the conformance tests (Spike A and later). `manifest.toml` lists every model, where it comes from and its SHA-256 hash.

## Commands

```
cargo xtask corpus list           show every entry and collection
cargo xtask corpus verify         check available files against their hashes
cargo xtask corpus verify --all   also fail if NetLogo or a download is missing
cargo xtask corpus fetch          download `url` entries into corpus/.cache, then verify
```

`verify` without `--all` skips entries whose NetLogo install isn't present, so it passes on machines and CI jobs without NetLogo. Set `LOGOS_NETLOGO_HOME` if NetLogo is installed somewhere other than the default location.

## Manifest format (version 1)

```toml
version = 1

[[model]]
id = "aa4-combined"              # lowercase letters, digits, `-` and `.`; unique
set = "C3"                       # corpus set from docs/phase-0-spec.md
source = "repo"                  # "repo" | "netlogo-install" | "url"
path = "own/aa4-combined/AA4-Combined.nlogox"
sha256 = "d01ec708..."           # 64 lowercase hex characters
license = "CC-BY-NC-SA-3.0"      # SPDX identifier where one exists
netlogo = "7.0.4"                # required for source = "netlogo-install"
url = "https://..."              # required for source = "url", not allowed otherwise
tags = ["primary", "file-io"]    # optional
notes = "..."                    # optional

[[collection]]                   # many files, no per-file hashes
id = "nl704.models-library"
set = "C1"
source = "netlogo-install"       # the only supported source for collections
netlogo = "7.0.4"
root = "models"
extensions = ["nlogox"]
license = "various"
notes = "..."                    # optional
```

How `path` is resolved:

| `source` | `path` is relative to |
|---|---|
| `repo` | this `corpus/` folder |
| `netlogo-install` | the NetLogo install folder for the `netlogo` version |
| `url` | `corpus/.cache/<id>/` (a plain file name) |

Paths always use `/`, even on Windows, and may not contain `..`.

## Reference results

`reference/` holds output produced by NetLogo itself, used as the expected values in conformance tests. Each file is listed in `manifest.toml` with the tag `reference`.

`aa4-n3-limit1-netlogo-7.0.4.csv` was produced with NetLogo 7.0.4's headless BehaviorSpace on Windows, on NetLogo's bundled Java runtime. `netlogo-headless.bat` in 7.0.4 fails when NetLogo is installed in a path with spaces (see `docs/decisions.md`, D-17), so Java was called directly:

```powershell
$NL = "C:\Program Files\NetLogo 7.0.4"
& "$NL\runtime\bin\java.exe" -Xmx4g "-Dfile.encoding=UTF-8" "-Djava.awt.headless=true" `
  "-Dnetlogo.extensions.dir=$NL\extensions" "-Dnetlogo.models.dir=$NL\models" "-Dnetlogo.docs.dir=$NL" `
  --add-exports=java.base/java.lang=ALL-UNNAMED --add-exports=java.desktop/sun.awt=ALL-UNNAMED --add-exports=java.desktop/sun.java2d=ALL-UNNAMED `
  -cp "$NL\app\netlogo-7.0.4.jar" org.nlogo.headless.Main `
  --model "<repo>\corpus\own\aa4-combined\AA4-Combined.nlogox" `
  --experiment N3-limit1 `
  --table "<repo>\corpus\reference\aa4-n3-limit1-netlogo-7.0.4.csv"
```

Headless BehaviorSpace does not update plots unless `--update-plots` is given; this file was produced without it.

## Rules

- Files under `corpus/` are test fixtures. Don't edit them; add a new entry instead. `.gitattributes` stops git from changing their line endings.
- Every model committed here has a `LICENSE.md` in its folder.
- Models from the Models Library and CoMSES are referenced, not committed.
- Changing a hash means the reference results recorded for that model are no longer valid. Re-record them in the same PR.
