// Experiments (design 3.3, 3.4): the Experiments panel, the experiment document with
// BehaviorSpace's settings and the Logos-only settings (D-27), the run dashboard, and results.

import { useEffect, useRef, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { signal } from "@preact/signals";
import { Icon } from "./icons";
import { errorText } from "./ipc";
import { ask } from "@tauri-apps/plugin-dialog";
import { addActivity, closeDoc, dirty, docs, findings, model, openDoc, saveModel, showToast } from "./state";
import { type WidgetEdit, applyResult, canRedo, canUndo, undoInterface } from "./editor";
import { joinLiterals, splitLiterals, steppedCount } from "./bspace";

interface LogosSettings { workers: number; updatePlots: boolean; outputs: string[] }
interface ExperimentView {
  name: string; repetitions: number; sequentialRunOrder: boolean; runMetricsEveryStep: boolean;
  setup: string; go: string; timeLimit: number; exitCondition: string; metrics: string[];
  variables: [string, string[]][]; runCount: number; settings: LogosSettings;
  parts: string[]; valueSets: ValueSet[];
}
interface RunRow { run: number; settings: [string, string][]; status: string; steps: number; seconds: number; worker?: number | null; errors: string[] }
interface StudyView {
  id: number; name: string; outDir: string; state: string; total: number; done: number; failed: number;
  workers: number; elapsed: number; runs: RunRow[]; error?: string | null; parquet?: string | null; resumed: number;
}
interface PastRun { dir: string; experiment: string; created: string; runCount: number; saved: number; complete: boolean }

export const experiments = signal<ExperimentView[] | null>(null);
export const past = signal<PastRun[]>([]);
/** Runs started in this session, by id: (experiment name, latest view). */
export const studies = signal<Record<number, StudyView>>({});

let listening = false;
function listenStudies() {
  if (listening) return;
  listening = true;
  listen<{ id: number }>("study", async (e) => {
    try {
      const v = await invoke<StudyView>("study_status", { id: e.payload.id });
      studies.value = { ...studies.value, [v.id]: v };
      if (v.state === "finished" || v.state === "stopped" || v.state === "failed") refreshExperiments();
    } catch { /* gone */ }
  });
}

export async function refreshExperiments() {
  const m = model.value;
  if (!m) return;
  try {
    experiments.value = await invoke<ExperimentView[]>("experiments_list", { id: m.id });
    past.value = await invoke<PastRun[]>("results_past", { id: m.id });
  } catch (e) {
    experiments.value = [];
    showToast(`Experiments: ${errorText(e)}`);
  }
}

export function ResultsPanel() {
  const m = model.value;
  useEffect(() => { listenStudies(); refreshExperiments(); }, [m?.id]);
  if (!m) return <p class="empty-note">Open a model to see its experiment results.</p>;
  if (past.value.length === 0) return <p class="empty-note">No results yet. Run an experiment from the Experiments panel; its results go in the results folder next to the model.</p>;
  return (
    <ul class="tree" aria-label="Results folders">
      {past.value.map((p) => (
        <li><button title={p.dir} onClick={() => openDoc({ id: `res-${p.dir}`, kind: "dashboard", runId: -1, name: p.experiment, dir: p.dir })}>
          <span class="kind">{p.complete ? "done" : `${p.saved}/${p.runCount}`}</span>
          <span>{p.experiment}</span>
          <span class="detail">{p.created.slice(0, 16).replace("T", " ")}</span>
        </button></li>
      ))}
    </ul>
  );
}

export function ExperimentsPanel() {
  const m = model.value;
  useEffect(() => { listenStudies(); refreshExperiments(); }, [m?.id]);
  if (!m) return <p class="empty-note">Open a model to see its BehaviorSpace experiments.</p>;
  const list = experiments.value;
  const running = Object.values(studies.value).filter((s) => ["starting", "running", "stopping"].includes(s.state));
  return (
    <div>
      {running.length > 0 && (
        <>
          <div class="panel-head" style="padding: 4px 6px 0"><span>Running</span></div>
          <ul class="tree">
            {running.map((s) => (
              <li><button onClick={() => openDoc({ id: `dash-${s.id}`, kind: "dashboard", runId: s.id, name: s.name })}>
                <span class="kind">{s.done}/{s.total}</span><span>{s.name}</span>
              </button></li>
            ))}
          </ul>
        </>
      )}
      <div class="panel-head" style="padding: 4px 6px 0"><span>In the model</span></div>
      {list === null ? <p class="empty-note">Reading…</p> : list.length === 0
        ? <p class="empty-note">This model has no BehaviorSpace experiments.</p>
        : (
          <ul class="tree" aria-label="Experiments">
            {list.map((e) => (
              <li><button onClick={() => openDoc({ id: `exp-${e.name}`, kind: "experiment", name: e.name })}>
                <span class="kind">{e.runCount} runs</span><span>{e.name}</span>
              </button></li>
            ))}
          </ul>
        )}
      {past.value.length > 0 && (
        <>
          <div class="panel-head" style="padding: 8px 6px 0"><span>Results</span></div>
          <ul class="tree" aria-label="Results folders">
            {past.value.map((p) => (
              <li><button title={p.dir} onClick={() => openDoc({ id: `res-${p.dir}`, kind: "dashboard", runId: -1, name: p.experiment, dir: p.dir })}>
                <span class="kind">{p.complete ? "done" : `${p.saved}/${p.runCount}`}</span>
                <span>{p.experiment}</span>
                <span class="detail">{p.created.slice(0, 16).replace("T", " ")}</span>
              </button></li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}

// ---- the experiment document (design 3.3) -------------------------------------------------

type ValueSet =
  | { kind: "enumerated"; variable: string; values: string[] }
  | { kind: "stepped"; variable: string; first: string; step: string; last: string };

type ExperimentOp =
  | { op: "attr"; name: string; attr: string; value: string }
  | { op: "text"; name: string; part: string; text: string }
  | { op: "timeLimit"; name: string; steps: number }
  | { op: "metrics"; name: string; metrics: string[] }
  | { op: "values"; name: string; variable: string; values: string[] }
  | { op: "stepped"; name: string; variable: string; first: string; step: string; last: string }
  | { op: "removeVariable"; name: string; variable: string }
  | { op: "duplicate"; name: string; newName: string }
  | { op: "delete"; name: string };

async function editExperiment(op: ExperimentOp, what: string): Promise<boolean> {
  const m = model.value;
  if (!m) return false;
  try {
    applyResult(await invoke<WidgetEdit>("experiment_edit", { id: m.id, op }));
    addActivity(what);
    await refreshExperiments();
    return true;
  } catch (e) {
    showToast(`Can't change the experiment: ${errorText(e)}`);
    return false;
  }
}

/** A text field that commits when it loses focus or on Enter (Ctrl+Enter in a text area). */
function Field({ id, value, onCommit, area, mono = true, width, disabled, label }: {
  id: string; value: string; onCommit: (v: string) => void; area?: boolean; mono?: boolean; width?: string;
  disabled?: boolean; label?: string;
}) {
  const [v, setV] = useState(value);
  useEffect(() => setV(value), [value]);
  const commit = () => { if (v !== value) onCommit(v); };
  const cls = mono ? "mono" : undefined;
  return area
    ? <textarea id={id} class={cls} rows={Math.min(8, Math.max(2, v.split("\n").length))} value={v} disabled={disabled} aria-label={label}
        onInput={(e) => setV((e.target as HTMLTextAreaElement).value)} onBlur={commit}
        onKeyDown={(e) => { if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) commit(); }} />
    : <input id={id} type="text" class={cls} style={width ? `width: ${width}` : undefined} value={v} disabled={disabled} aria-label={label}
        onInput={(e) => setV((e.target as HTMLInputElement).value)} onBlur={commit}
        onKeyDown={(e) => { if (e.key === "Enter") commit(); }} />;
}

function VariableRow({ exp, vs }: { exp: string; vs: ValueSet }) {
  const [problem, setProblem] = useState<string | null>(null);
  const count = vs.kind === "enumerated" ? vs.values.length : steppedCount(vs.first, vs.step, vs.last);
  const setValues = (line: string) => {
    const parts = splitLiterals(line);
    if (typeof parts === "string") { setProblem(parts); return; }
    if (parts.length === 0) { setProblem("A variable needs at least one value. Remove it instead."); return; }
    setProblem(null);
    editExperiment({ op: "values", name: exp, variable: vs.variable, values: parts }, `Changed the values of ${vs.variable} in ${exp}`);
  };
  const setStepped = (k: "first" | "step" | "last", v: string) => {
    if (vs.kind !== "stepped") return;
    if (!Number.isFinite(Number(v))) { setProblem(`${k} must be a number.`); return; }
    setProblem(null);
    editExperiment({ op: "stepped", name: exp, variable: vs.variable, first: vs.first, step: vs.step, last: vs.last, [k]: v },
      `Changed the ${k} value of ${vs.variable} in ${exp}`);
  };
  return (
    <tr>
      <td class="mono">{vs.variable}</td>
      <td>
        {vs.kind === "enumerated"
          ? <Field id={`v-${vs.variable}`} label={`Values of ${vs.variable}`} value={joinLiterals(vs.values)} onCommit={setValues} width="100%" />
          : <span class="stepped">
              <label>first <Field id={`f-${vs.variable}`} label={`First value of ${vs.variable}`} value={vs.first} onCommit={(v) => setStepped("first", v)} width="70px" /></label>
              <label>step <Field id={`s-${vs.variable}`} label={`Increment of ${vs.variable}`} value={vs.step} onCommit={(v) => setStepped("step", v)} width="70px" /></label>
              <label>last <Field id={`l-${vs.variable}`} label={`Last value of ${vs.variable}`} value={vs.last} onCommit={(v) => setStepped("last", v)} width="70px" /></label>
            </span>}
        {problem && <div class="err" style="font-size: 12px" role="alert">{problem}</div>}
      </td>
      <td>{count ?? "?"}</td>
      <td><button class="icon-btn" aria-label={`Remove ${vs.variable} from the experiment`} title="Remove"
        onClick={() => editExperiment({ op: "removeVariable", name: exp, variable: vs.variable }, `Removed ${vs.variable} from ${exp}`)}>
        <Icon name="close" /></button></td>
    </tr>
  );
}

function AddVariable({ exp, present }: { exp: string; present: string[] }) {
  const globals = (model.value?.widgets ?? [])
    .map((w) => ("variable" in w.data ? w.data.variable : ""))
    .filter((v) => v && !present.some((p) => p.toLowerCase() === v.toLowerCase()));
  const [variable, setVariable] = useState("");
  const [values, setValues] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const add = () => {
    const name = variable.trim();
    if (!name) { setProblem("Choose or type a variable."); return; }
    const parts = splitLiterals(values);
    if (typeof parts === "string") { setProblem(parts); return; }
    if (parts.length === 0) { setProblem("Type at least one value."); return; }
    setProblem(null);
    editExperiment({ op: "values", name: exp, variable: name, values: parts }, `Added ${name} to ${exp}`).then((ok) => {
      if (ok) { setVariable(""); setValues(""); }
    });
  };
  return (
    <tr>
      <td>
        <input type="text" class="mono" list="exp-globals" placeholder="variable" aria-label="Variable to add" value={variable}
          style="width: 100%" onInput={(e) => setVariable((e.target as HTMLInputElement).value)} />
        <datalist id="exp-globals">{globals.map((g) => <option value={g} />)}</datalist>
      </td>
      <td>
        <input type="text" class="mono" placeholder='values, e.g. 0.1 0.2 0.3 or "a" "b"' aria-label="Values of the new variable" value={values}
          style="width: 100%" onInput={(e) => setValues((e.target as HTMLInputElement).value)}
          onKeyDown={(e) => { if (e.key === "Enter") add(); }} />
        {problem && <div class="err" style="font-size: 12px" role="alert">{problem}</div>}
      </td>
      <td />
      <td><button class="btn small" onClick={add}>Add</button></td>
    </tr>
  );
}

export function ExperimentDoc({ name, docId }: { name: string; docId?: string }) {
  const m = model.value!;
  const e = experiments.value?.find((x) => x.name === name);
  const [s, setS] = useState<LogosSettings | null>(e?.settings ?? null);
  const [copyName, setCopyName] = useState<string | null>(null);
  useEffect(() => { if (e) setS(e.settings); }, [e?.name]);
  if (experiments.value && !e) return <div class="doc-page"><p class="muted">The experiment {name} is no longer in the model.</p></div>;
  if (!e || !s) return <div class="doc-page"><p class="muted">Reading the experiment…</p></div>;
  const previous = past.value.filter((p) => p.experiment === name && p.complete);
  const errors = findings.value.filter((f) => f.severity === "error");
  const has = (part: string) => e.parts.includes(part);
  const update = (patch: Partial<LogosSettings>) => {
    const next = { ...s, ...patch };
    setS(next);
    invoke("experiment_save_settings", { id: m.id, name, settings: next }).catch((er) => showToast(errorText(er)));
  };
  const run = async () => {
    if (dirty.value) {
      await saveModel();
    }
    try {
      const id = await invoke<number>("experiment_run", { id: m.id, name, settings: s, resume: null });
      addActivity(`Started experiment ${name} (${e.runCount} runs, ${s.workers} workers)`);
      openDoc({ id: `dash-${id}`, kind: "dashboard", runId: id, name });
    } catch (er) { showToast(errorText(er)); }
  };
  const rename = async (to: string) => {
    const next = to.trim();
    if (!next || next === name) return;
    if (await editExperiment({ op: "attr", name, attr: "name", value: next }, `Renamed experiment ${name} to ${next}`)) {
      // The Logos settings follow the name (D-27); the old entry stays until the model is saved.
      invoke("experiment_save_settings", { id: m.id, name: next, settings: s }).catch(() => {});
      if (docId) docs.value = docs.value.map((d) => (d.id === docId && d.kind === "experiment" ? { ...d, name: next } : d));
    }
  };
  const duplicate = async () => {
    const to = (copyName ?? "").trim();
    if (!to) return;
    if (await editExperiment({ op: "duplicate", name, newName: to }, `Copied experiment ${name} to ${to}`)) {
      invoke("experiment_save_settings", { id: m.id, name: to, settings: s }).catch(() => {});
      setCopyName(null);
      openDoc({ id: `exp-${to}-${Date.now()}`, kind: "experiment", name: to });
    }
  };
  const remove = async () => {
    const ok = await ask(`Delete the experiment ${name} from the model? Undo (Ctrl+Z) brings it back until you close the model.`,
      { title: "Delete experiment", kind: "warning", okLabel: "Delete", cancelLabel: "Cancel" });
    if (ok && (await editExperiment({ op: "delete", name }, `Deleted experiment ${name}`)) && docId) closeDoc(docId);
  };
  const missing = (part: string) => <span class="muted" style="font-size: 12px">Not set in this experiment. Set it once in NetLogo's BehaviorSpace dialog; Logos can't add this element yet.</span>;
  return (
    <div class="doc-page experiment-editor">
      <div style="display: flex; align-items: center; gap: 8px; flex-wrap: wrap">
        <label for="exp-name" class="sr-only">Experiment name</label>
        <Field id="exp-name" value={e.name} onCommit={rename} mono={false} width="260px" />
        <span class="muted">{e.runCount} runs</span>
        <span style="flex: 1" />
        <button class="icon-btn" aria-label="Undo" title="Undo (Ctrl+Z)" disabled={!canUndo.value} onClick={() => undoInterface(false)}>↶</button>
        <button class="icon-btn" aria-label="Redo" title="Redo (Ctrl+Y)" disabled={!canRedo.value} onClick={() => undoInterface(true)}>↷</button>
        {copyName === null
          ? <button class="btn small" onClick={() => setCopyName(`${name} copy`)}>Duplicate</button>
          : <span style="display: flex; gap: 4px">
              <input type="text" aria-label="Name of the copy" value={copyName} autoFocus
                onInput={(ev) => setCopyName((ev.target as HTMLInputElement).value)}
                onKeyDown={(ev) => { if (ev.key === "Enter") duplicate(); if (ev.key === "Escape") setCopyName(null); }} />
              <button class="btn small" onClick={duplicate}>Copy</button>
            </span>}
        <button class="btn small" onClick={remove}>Delete</button>
      </div>
      <p class="muted" style="font-size: 12px">BehaviorSpace settings, stored in the model. Changes are unsaved until you save the model, and undo like interface changes.</p>

      <h3>Vary variables as follows</h3>
      <table class="grid-table">
        <thead><tr><th style="width: 22%">Variable</th><th>Values (NetLogo literals)</th><th style="width: 60px">Count</th><th style="width: 60px" /></tr></thead>
        <tbody>
          {e.valueSets.map((vs) => <VariableRow exp={e.name} vs={vs} />)}
          {has("constants") && <AddVariable exp={e.name} present={e.valueSets.map((v) => v.variable)} />}
        </tbody>
      </table>

      <div class="form-grid" style="margin-top: 12px; grid-template-columns: 200px 1fr">
        <label for="exp-reps">Repetitions</label>
        <div><Field id="exp-reps" value={String(e.repetitions)} width="80px"
          onCommit={(v) => { const n = Math.floor(Number(v)); if (n >= 1) editExperiment({ op: "attr", name, attr: "repetitions", value: String(n) }, `Set repetitions of ${name} to ${n}`); else showToast("Repetitions must be 1 or more."); }} /></div>
        <label for="exp-seq">Run combinations in sequential order</label>
        <div><input id="exp-seq" type="checkbox" checked={e.sequentialRunOrder}
          onChange={(ev) => editExperiment({ op: "attr", name, attr: "sequentialRunOrder", value: String((ev.target as HTMLInputElement).checked) }, `Changed the run order of ${name}`)} /></div>
        <label for="exp-metrics">Measure runs using these reporters (one per line)</label>
        <div><Field id="exp-metrics" area value={e.metrics.join("\n")}
          onCommit={(v) => editExperiment({ op: "metrics", name, metrics: v.split("\n").map((x) => x.trim()).filter(Boolean) }, `Changed the metrics of ${name}`)} /></div>
        <label for="exp-every">Measure runs at every step</label>
        <div><input id="exp-every" type="checkbox" checked={e.runMetricsEveryStep}
          onChange={(ev) => editExperiment({ op: "attr", name, attr: "runMetricsEveryStep", value: String((ev.target as HTMLInputElement).checked) }, `Changed when ${name} measures`)} /></div>
        <label for="exp-setup">Setup commands</label>
        <div>{has("setup") ? <Field id="exp-setup" area value={e.setup} onCommit={(v) => editExperiment({ op: "text", name, part: "setup", text: v }, `Changed the setup commands of ${name}`)} /> : missing("setup")}</div>
        <label for="exp-go">Go commands</label>
        <div>{has("go") ? <Field id="exp-go" area value={e.go} onCommit={(v) => editExperiment({ op: "text", name, part: "go", text: v }, `Changed the go commands of ${name}`)} /> : missing("go")}</div>
        <label for="exp-stop">Stop condition</label>
        <div>{has("exitCondition") ? <Field id="exp-stop" area value={e.exitCondition} onCommit={(v) => editExperiment({ op: "text", name, part: "exitCondition", text: v }, `Changed the stop condition of ${name}`)} /> : missing("exitCondition")}</div>
        <label for="exp-limit">Time limit (steps, 0 for none)</label>
        <div>{has("timeLimit") ? <Field id="exp-limit" value={String(e.timeLimit)} width="100px"
          onCommit={(v) => { const n = Math.floor(Number(v)); if (n >= 0) editExperiment({ op: "timeLimit", name, steps: n }, `Set the time limit of ${name} to ${n}`); }} /> : missing("timeLimit")}</div>
      </div>

      <h3>Logos settings</h3>
      <p class="muted">Saved in <span class="mono">{m.name.replace(/\.nlogox$/i, "")}.experiments.toml</span> next to the model, not in the model (D-27).</p>
      <div class="form-grid">
        <label for="workers">Workers</label>
        <div><input id="workers" type="number" min={1} max={64} value={s.workers} style="width: 80px"
          onChange={(ev) => update({ workers: Math.max(1, Number((ev.target as HTMLInputElement).value) || 1) })} />
          <span class="muted"> parallel runs</span></div>
        <label for="plots">Plot updates</label>
        <div><label><input id="plots" type="checkbox" checked={s.updatePlots} onChange={(ev) => update({ updatePlots: (ev.target as HTMLInputElement).checked })} /> update plots during runs <span class="muted">(off in headless BehaviorSpace)</span></label></div>
        <label>Output</label><div>NetLogo's table (table.csv), Parquet (results.parquet), and a run record per run</div>
      </div>
      <div class="card" style="margin-top: 16px; display: flex; align-items: center; gap: 16px; flex-wrap: wrap">
        <span><strong>{e.runCount}</strong> runs</span>
        {previous.length > 0 && <span class="muted">Ran before: {previous.length} time{previous.length > 1 ? "s" : ""}</span>}
        <span class={errors.length ? "err" : "ok"}>{errors.length ? `✕ ${errors.length} problem${errors.length > 1 ? "s" : ""} from logos check` : "✓ logos check found no errors"}</span>
        {dirty.value && <span class="muted">The model is saved before the run starts.</span>}
        <span style="flex:1" />
        <button class="btn primary" onClick={run}><Icon name="play" /> Run</button>
      </div>
    </div>
  );
}

// ---- the run dashboard and results (design 3.4) -------------------------------------------

function fmtTime(s: number): string {
  if (!Number.isFinite(s) || s < 0) return "–";
  const m = Math.floor(s / 60), sec = Math.round(s % 60);
  return m ? `${m} min ${sec} s` : `${sec} s`;
}

export function DashboardDoc({ runId, dir }: { runId: number; dir?: string }) {
  const [tab, setTab] = useState<"runs" | "results">("runs");
  const [v, setV] = useState<StudyView | null>(runId >= 0 ? studies.value[runId] ?? null : null);
  const [results, setResults] = useState<ResultsData | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const resultDir = runId < 0 ? dir : v?.outDir;
  useEffect(() => {
    if (runId < 0) return;
    let stop = false;
    const tick = async () => {
      try {
        const s = await invoke<StudyView>("study_status", { id: runId });
        if (stop) return;
        setV(s);
        studies.value = { ...studies.value, [s.id]: s };
        if (["starting", "running", "stopping"].includes(s.state)) setTimeout(tick, 500);
      } catch { /* gone */ }
    };
    tick();
    return () => { stop = true; };
  }, [runId]);
  useEffect(() => {
    if (!resultDir) return;
    if (runId >= 0 && v && !["finished", "stopped", "failed"].includes(v.state)) return;
    invoke<ResultsData>("results_read", { dir: resultDir }).then(setResults).catch(() => setResults(null));
  }, [resultDir, v?.state]);

  if (runId >= 0 && !v) return <div class="doc-page"><p class="muted">Starting…</p></div>;
  const live = runId >= 0 && v ? v : null;
  const rows: RunRow[] = live ? live.runs : (results?.runs ?? []).map((r) => ({
    run: r.run, settings: r.settings, status: r.reason === "Failed" || r.errors.length ? "failed" : "done",
    steps: r.steps, seconds: r.seconds, worker: r.record?.worker ?? null, errors: r.errors,
  }));
  const total = live ? live.total : results?.meta.run_count ?? rows.length;
  const done = live ? live.done : rows.length;
  const failed = live ? live.failed : rows.filter((r) => r.status === "failed").length;
  const elapsed = live ? live.elapsed : 0;
  const perRun = done > (live?.resumed ?? 0) ? elapsed / (done - (live?.resumed ?? 0)) : NaN;
  const remaining = live && ["running", "starting"].includes(live.state) ? perRun * (total - done) : NaN;
  const state = live ? live.state : results ? (rows.length >= total ? "finished" : "stopped") : "reading";
  const name = live?.name ?? results?.meta.experiment ?? "";
  // Show the variables that change between runs; the others are in each run's record.
  const allParams = rows[0]?.settings.map(([n]) => n) ?? [];
  const variedIdx = allParams.map((_, i) => i).filter((i) => new Set(rows.map((r) => r.settings[i]?.[1])).size > 1);
  const shownIdx = variedIdx.length ? variedIdx : allParams.map((_, i) => i).slice(0, 4);
  const params = shownIdx.map((i) => allParams[i]);
  const pick = (r: RunRow) => shownIdx.map((i) => r.settings[i] ?? ["", ""]);
  const running = rows.filter((r) => r.status === "running");
  const sel = rows.find((r) => r.run === selected);
  return (
    <div class="doc-page">
      <h2>{name} <span class="muted" style="font-weight: 400; font-size: 14px">{state}</span></h2>
      <div role="status">
        <span class="stat"><b>{done}</b><span>of {total} runs done</span></span>
        <span class="stat"><b class={failed ? "err" : ""}>{failed}</b><span>with errors</span></span>
        <span class="stat"><b>{Math.max(0, total - done)}</b><span>remaining</span></span>
        {live && <span class="stat"><b>{fmtTime(elapsed)}</b><span>elapsed</span></span>}
        {live && Number.isFinite(remaining) && <span class="stat"><b>{fmtTime(remaining)}</b><span>estimated left</span></span>}
      </div>
      <div class="bar" role="progressbar" aria-valuemin={0} aria-valuemax={total} aria-valuenow={done} aria-label="Runs done">
        <span style={{ width: `${total ? (100 * done) / total : 0}%` }} />
      </div>
      <div class="actions" style="margin: 10px 0">
        {live && ["running", "starting"].includes(live.state) && (
          <button class="btn" onClick={() => invoke("study_stop", { id: live.id }).catch((e) => showToast(errorText(e)))}>
            <Icon name="stop" /> Stop
          </button>
        )}
        {state === "stopped" && resultDir && model.value && (
          <button class="btn primary" onClick={async () => {
            const m = model.value!;
            const e = experiments.value?.find((x) => x.name === name);
            try {
              const id = await invoke<number>("experiment_run", { id: m.id, name, settings: e?.settings ?? { workers: 4, updatePlots: false, outputs: [] }, resume: resultDir });
              openDoc({ id: `dash-${id}`, kind: "dashboard", runId: id, name });
            } catch (er) { showToast(errorText(er)); }
          }}>Resume</button>
        )}
        {resultDir && <button class="btn" onClick={() => revealItemInDir(resultDir).catch(() => {})}>Open results folder</button>}
        <span class="muted mono" style="font-size: 12px">{resultDir}</span>
      </div>
      {live?.error && <p class="err" style="white-space: pre-wrap">{live.error}</p>}
      {live && running.length > 0 && (
        <>
          <h3>Workers</h3>
          <p class="muted" style="margin: 0 0 4px; font-size: 12px">Runs in progress. Step-by-step progress per worker comes with the bridge's progress events (G11).</p>
          <table class="grid-table">
            <thead><tr><th>Run</th>{params.map((p) => <th>{p}</th>)}</tr></thead>
            <tbody>{running.map((r) => <tr><td>{r.run}</td>{pick(r).map(([, x]) => <td class="mono">{x}</td>)}</tr>)}</tbody>
          </table>
        </>
      )}
      <div class="subtabs" role="tablist" style="margin-top: 14px; padding: 0">
        <button role="tab" aria-selected={tab === "runs"} onClick={() => setTab("runs")}>Runs</button>
        <button role="tab" aria-selected={tab === "results"} onClick={() => setTab("results")} disabled={!results}>Results</button>
      </div>
      {tab === "runs" ? (
        <div style="display: grid; grid-template-columns: minmax(0, 2fr) minmax(0, 1fr); gap: 14px; margin-top: 8px">
          <div style="overflow: auto; max-height: 460px">
            <table class="grid-table" aria-label="Runs">
              <thead><tr><th>Run</th>{params.map((p) => <th>{p}</th>)}<th>Status</th><th>Steps</th><th>Time</th></tr></thead>
              <tbody>
                {rows.map((r) => (
                  <tr class={`clickable${selected === r.run ? " selected" : ""}`} onClick={() => setSelected(r.run)} tabIndex={0}
                    onKeyDown={(e) => { if (e.key === "Enter") setSelected(r.run); }}>
                    <td>{r.run}</td>
                    {pick(r).map(([, x]) => <td class="mono">{x}</td>)}
                    <td class={r.status === "failed" ? "err" : r.status === "running" ? "" : r.status === "done" ? "ok" : "muted"}>
                      {r.status === "failed" ? "✕ errors" : r.status === "running" ? "● running" : r.status}
                    </td>
                    <td>{r.steps || ""}</td>
                    <td>{r.seconds ? `${r.seconds.toFixed(1)} s` : ""}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <RunRecord row={sel} results={results} />
        </div>
      ) : results && <ResultsView data={results} varied={params} />}
    </div>
  );
}

interface ResultsData {
  meta: { experiment: string; run_count: number; variables: string[]; metrics: string[]; created: string };
  columns: string[];
  rows: (number | string | null)[][];
  totalRows: number;
  runs: { run: number; steps: number; seconds: number; reason: string; settings: [string, string][]; errors: string[];
    record?: { worker?: number; random_seed?: string | null; started?: string; ended?: string; provenance?: unknown } | null }[];
}

function RunRecord({ row, results }: { row?: RunRow; results: ResultsData | null }) {
  if (!row) return <div class="card"><p class="muted" style="margin: 0">Select a run to see its record.</p></div>;
  const saved = results?.runs.find((r) => r.run === row.run);
  const rec = saved?.record;
  return (
    <div class="card" aria-label={`Run ${row.run}`}>
      <h3 style="margin-top: 0">Run {row.run}</h3>
      <dl class="kv">
        {row.settings.map(([n, v]) => <><dt>{n}</dt><dd>{v}</dd></>)}
        <dt>Steps</dt><dd>{row.steps}</dd>
        <dt>Time</dt><dd>{row.seconds.toFixed(2)} s</dd>
        {row.worker != null && <><dt>Worker</dt><dd>{row.worker}</dd></>}
        {rec?.random_seed && <><dt>random-seed</dt><dd>{rec.random_seed}</dd></>}
        {rec?.started && <><dt>Started</dt><dd>{rec.started}</dd></>}
        {rec?.ended && <><dt>Ended</dt><dd>{rec.ended}</dd></>}
      </dl>
      {row.errors.length > 0 && (
        <>
          <h3>Errors</h3>
          <ul class="findings err">{row.errors.map((e) => <li>{e}</li>)}</ul>
        </>
      )}
      {rec?.provenance != null && (
        <details style="margin-top: 8px"><summary>Provenance</summary>
          <pre class="log">{JSON.stringify(rec.provenance, null, 2)}</pre></details>
      )}
    </div>
  );
}

function ResultsView({ data, varied }: { data: ResultsData; varied: string[] }) {
  const metrics = data.meta.metrics;
  const [metric, setMetric] = useState(metrics[0] ?? "");
  // With measurements only at the end of each run, a varied variable is the useful x axis.
  const steps = new Set(data.rows.map((r) => r[data.columns.indexOf("[step]")])).size;
  const [x, setX] = useState(steps > data.runs.length || varied.length === 0 ? "[step]" : varied[0]);
  const ref = useRef<HTMLCanvasElement>(null);
  const col = (n: string) => data.columns.indexOf(n);
  useEffect(() => {
    const c = ref.current;
    if (!c) return;
    const rect = c.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    c.width = Math.floor(rect.width * dpr); c.height = Math.floor(rect.height * dpr);
    const ctx = c.getContext("2d")!;
    ctx.scale(dpr, dpr);
    const css = getComputedStyle(document.documentElement);
    const tok = (n: string) => css.getPropertyValue(n).trim();
    const colors = ["--syn-number", "--syn-string", "--syn-keyword", "--syn-procedure", "--accent", "--ok", "--warning", "--error"].map(tok);
    ctx.clearRect(0, 0, rect.width, rect.height);
    const run = col("[run number]"), xi = col(x), yi = col(metric);
    if (run < 0 || xi < 0 || yi < 0) return;
    const series = new Map<number, [number, number][]>();
    for (const r of data.rows) {
      const xv = Number(r[xi]), yv = Number(r[yi]);
      if (!Number.isFinite(xv) || !Number.isFinite(yv)) continue;
      const k = Number(r[run]);
      if (!series.has(k)) series.set(k, []);
      series.get(k)!.push([xv, yv]);
    }
    const all = [...series.values()].flat();
    if (all.length === 0) {
      ctx.fillStyle = tok("--text-muted"); ctx.font = "12px sans-serif";
      ctx.fillText("No numeric values for this metric.", 50, 30);
      return;
    }
    const [x0, x1] = [Math.min(...all.map((p) => p[0])), Math.max(...all.map((p) => p[0]))];
    const [y0, y1] = [Math.min(...all.map((p) => p[1])), Math.max(...all.map((p) => p[1]))];
    const L = 56, R = 12, T = 10, B = 28, W = rect.width - L - R, H = rect.height - T - B;
    const sx = (v: number) => L + (x1 === x0 ? W / 2 : ((v - x0) / (x1 - x0)) * W);
    const sy = (v: number) => T + H - (y1 === y0 ? H / 2 : ((v - y0) / (y1 - y0)) * H);
    ctx.strokeStyle = tok("--control-border"); ctx.lineWidth = 1;
    ctx.beginPath(); ctx.moveTo(L, T); ctx.lineTo(L, T + H); ctx.lineTo(L + W, T + H); ctx.stroke();
    ctx.fillStyle = tok("--text-muted"); ctx.font = "11px sans-serif";
    ctx.fillText(String(+y1.toPrecision(4)), 4, T + 10); ctx.fillText(String(+y0.toPrecision(4)), 4, T + H);
    ctx.fillText(String(+x0.toPrecision(4)), L, T + H + 16);
    const xl = String(+x1.toPrecision(4)); ctx.fillText(xl, L + W - ctx.measureText(xl).width, T + H + 16);
    ctx.fillText(x, L + W / 2 - 20, T + H + 24);
    let i = 0;
    const lines = x === "[step]";
    for (const [, pts] of series) {
      ctx.strokeStyle = ctx.fillStyle = colors[i++ % colors.length];
      ctx.globalAlpha = series.size > 12 ? 0.55 : 1;
      if (lines && pts.length > 1) {
        ctx.beginPath();
        pts.forEach(([a, b], j) => (j ? ctx.lineTo(sx(a), sy(b)) : ctx.moveTo(sx(a), sy(b))));
        ctx.lineWidth = 1.5; ctx.stroke();
      } else {
        for (const [a, b] of pts) { ctx.beginPath(); ctx.arc(sx(a), sy(b), 2.5, 0, Math.PI * 2); ctx.fill(); }
      }
    }
    ctx.globalAlpha = 1;
  }, [data, metric, x]);
  const xs = ["[step]", ...data.meta.variables];
  return (
    <div style="margin-top: 8px">
      <div class="actions">
        <label>Metric <select value={metric} onChange={(e) => setMetric((e.target as HTMLSelectElement).value)}>
          {metrics.map((m) => <option value={m}>{m}</option>)}</select></label>
        <label>Against <select value={x} onChange={(e) => setX((e.target as HTMLSelectElement).value)}>
          {xs.map((m) => <option value={m}>{m === "[step]" ? "step (one line per run)" : m}</option>)}</select></label>
      </div>
      <canvas ref={ref} style="width: 100%; height: 300px; margin-top: 8px" role="img"
        aria-label={`${metric} against ${x}, ${data.runs.length} runs`} />
      <h3>Table <span class="muted" style="font-weight: 400">({data.totalRows} rows{data.totalRows > data.rows.length ? `, first ${data.rows.length} shown` : ""}; results.parquet)</span></h3>
      <div style="overflow: auto; max-height: 360px">
        <table class="grid-table">
          <thead><tr>{data.columns.map((c) => <th class="mono">{c}</th>)}</tr></thead>
          <tbody>{data.rows.slice(0, 300).map((r) => <tr>{r.map((v) => <td class="mono">{v === null ? "" : typeof v === "number" ? +v.toPrecision(8) : v}</td>)}</tr>)}</tbody>
        </table>
      </div>
    </div>
  );
}
