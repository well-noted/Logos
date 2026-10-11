# Handoff: running the Phase 1 Part E tests

This is for a Claude Code session running in a terminal on Thomas's Windows machine. Your job is to run commands, record their output, and write a report. Don't change any file under `crates/`, `bridge/`, `docs/` or `protocol/`, and don't commit, push or switch branches except as written below. If a step fails, record the failure (the exact command and the last 30 lines of output) and go on to the next step. Don't try to fix code.

## Setup

- Repository: `C:\Users\tetuo\Logos`. Shell: PowerShell. Run every command from the repository folder unless a step says otherwise.
- NetLogo 7.0.4 is installed in `C:\Program Files\NetLogo 7.0.4`.
- Branch to test: `phase1/results-and-resume`.

```
cd C:\Users\tetuo\Logos
git status --short
git switch phase1/results-and-resume
git log --oneline -3
```

If `git status --short` lists changed files (lines not starting with `??`), stop and report it. Don't switch branches over uncommitted changes.

Set these for the whole session:

```
$env:LOGOS_NETLOGO_SRC = "C:\Users\tetuo\Logos\corpus\.cache\NetLogo-7.0.4"
$env:LOGOS_WORKERS = "8"
$L = "C:\Users\tetuo\Logos\target\debug\logos.exe"
$NLH = "C:\Program Files\NetLogo 7.0.4\netlogo-headless.bat"
$AA4 = "C:\Users\tetuo\Logos\corpus\own\aa4-combined"
```

## T1. Build and unit tests

```
cd bridge; .\gradlew.bat test jar; cd ..
cargo build -p logos-cli
cargo test --workspace
```

Record: whether the Gradle build ended with `BUILD SUCCESSFUL`, and every line of the cargo test output that starts with `test result:` or contains `FAILED`.

## T2. Conformance tests (E1, E2, A3, A6)

```
cargo test -p logos-experiment --test conformance -- --ignored --test-threads=1 --nocapture e1_netlogo a3_ a6_ 2>&1 | Tee-Object -FilePath $env:TEMP\t2.log
```

Takes about 5 minutes. Record:
- the number of lines containing `same` and the full text of any line containing `DIFFERS` or `E2`;
- the lines starting with `N3-limit1:` and `A6:`;
- the final `test result:` line.

## T3. Record NetLogo reference tables for AA4 (step 6)

For each of these seven experiments, run NetLogo's own headless BehaviorSpace and time it:

```
E3-holdout-cell1, E3-holdout-cell2, E3-holdout-cell3, E3-holdout-controls,
E3-holdout-retuned, E4-crowding-presentation, E3-crowding-sweep
```

Run them one at a time, in that order, with this loop (it takes one to two hours in total; let it run):

```
$names = "E3-holdout-cell1","E3-holdout-cell2","E3-holdout-cell3","E3-holdout-controls","E3-holdout-retuned","E4-crowding-presentation","E3-crowding-sweep"
foreach ($n in $names) {
  $out = "C:\Users\tetuo\Logos\corpus\reference\aa4-$($n.ToLower())-netlogo-7.0.4.csv"
  if (Test-Path $out) { Write-Output "$n exists, skipped"; continue }
  $t = Measure-Command { & $NLH --model "$AA4\AA4-Combined.nlogox" --experiment $n --table $out --threads 8 | Out-Null }
  Write-Output ("{0}: {1:N1} s, {2} lines" -f $n, $t.TotalSeconds, (Get-Content $out).Count)
}
```

Record each output line. Don't commit the CSV files; Thomas decides whether they go into the repository.

## T4. Compare Logos with the references (E1, E2 on AA4)

```
$env:LOGOS_AA4_REFERENCES = "C:\Users\tetuo\Logos\corpus\reference"
cargo test -p logos-experiment --test conformance -- --ignored --nocapture e1_aa4_all 2>&1 | Tee-Object -FilePath $env:TEMP\t4.log
```

Takes about as long as T3. Record each line that starts with an experiment name (`E3-...:`, `E4-...:`), any line starting with `  Logos` or `  NetLogo` or `  E2:` (at most 10), and the `test result:` line.

## T5. Resume (E4)

An uninterrupted run, then an interrupted and resumed one, then a comparison:

```
& $L run N3-limit1 $AA4 --workers 8 --out "$env:TEMP\e4-full"

$p = Start-Process -FilePath $L -ArgumentList @("run","N3-limit1",$AA4,"--workers","8","--out","$env:TEMP\e4-resume") -PassThru -NoNewWindow
Start-Sleep -Seconds 60
Stop-Process -Id $p.Id -Force
Start-Sleep -Seconds 5
(Get-ChildItem "$env:TEMP\e4-resume\runs").Count
& $L run --resume "$env:TEMP\e4-resume" --workers 8

$a = Get-Content "$env:TEMP\e4-full\table.csv" | Select-Object -Skip 7 | Sort-Object
$b = Get-Content "$env:TEMP\e4-resume\table.csv" | Select-Object -Skip 7 | Sort-Object
"rows: $($a.Count) and $($b.Count); differences: $((Compare-Object $a $b).Count)"
Get-ChildItem "$env:TEMP\e4-resume"
Get-Process java -ErrorAction SilentlyContinue | Select-Object Id, StartTime
```

Record: the number of run files after the kill, the last 4 lines printed by the resumed run, the `rows: ... differences: ...` line, the file listing, and whether any `java` processes are still running (there shouldn't be any from these runs; the bridge exits when Logos does).

Thomas tests Ctrl-C himself; don't try to send it.

## T6. Throughput (E5)

```
$t1 = Measure-Command { & $NLH --model "$AA4\AA4-Combined.nlogox" --experiment N3-limit1 --table "$env:TEMP\e5-netlogo.csv" --threads 8 | Out-Null }
$t2 = Measure-Command { & $L run N3-limit1 $AA4 --workers 8 --out "$env:TEMP\e5-logos" | Out-Null }
"NetLogo {0:N1} s, Logos {1:N1} s" -f $t1.TotalSeconds, $t2.TotalSeconds
```

Record the last line.

## Clean up

```
Remove-Item -Recurse -Force "$env:TEMP\e4-full","$env:TEMP\e4-resume","$env:TEMP\e5-logos","$env:TEMP\e5-netlogo.csv" -ErrorAction SilentlyContinue
git status --short
```

`git status` should list only the new reference CSVs under `corpus\reference` (as `??`) and possibly `results\` folders. Record it.

## Report

Write the report to `C:\Users\tetuo\Logos\test-reports\phase1-partE-<date>.md` (create the folder; it isn't committed), then print it. Use this form, one section per step, quoting output lines exactly:

```
# Part E test report, <date>

Branch and commit: <output of git log --oneline -1>

## T1 Build and unit tests
Gradle: <BUILD SUCCESSFUL or the failure>
<test result lines>

## T2 Conformance
...

## T3 References
...

## T4 AA4 comparison
...

## T5 Resume
...

## T6 Throughput
...

## Clean up
...

## Problems
<every failed command, with its last 30 lines of output; "none" if none>
```
