// Experiments (design 3.3, 3.4): the Experiments panel, the experiment document with
// BehaviorSpace's settings and the Logos-only settings (D-27), the run dashboard, and results.

import { useEffect, useRef, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { signal } from "@preact/signals";
import { Icon } from "./icons";
import { errorText } from "./ipc";
import { addActivity, dirty, findings, model, openDoc, saveModel, showToast } from "./state";

interface LogosSettings { workers: number; updatePlots: boolean; outputs: string[] }
interface ExperimentView {
  name: string; repetitions: number; sequentialRunOrder: boolean; runMetricsEveryStep: boolean;
  setup: string; go: string; timeLimit: number; exitCondition: string; metrics: string[];
  variables: [string, string[]][]; runCount: number; settings: LogosSettings;
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

export function ExperimentDoc({ name }: { name: string }) {
  const m = model.value!;
  const e = experiments.value?.find((x) => x.name === name);
  const [s, setS] = useState<LogosSettings | null>(e?.settings ?? null);
  useEffect(() => { if (e) setS(e.settings); }, [e?.name]);
  if (!e || !s) return <div class="doc-page"><p class="muted">Reading the experiment…</p></div>;
  const previous = past.value.filter((p) => p.experiment === name && p.complete);
  const errors = findings.value.filter((f) => f.severity === "error");
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
  const row = (label: string, v: string) => (
    <><label>{label}</label><div class="mono" style="white-space: pre-wrap">{v || <span class="muted">(none)</span>}</div></>
  );
  return (
    <div class="doc-page">
      <h2>{e.name}</h2>
      <p class="muted">BehaviorSpace settings, as stored in the model. Editing them here comes with the interface and experiment editing step (11 and 16); until then edit them in NetLogo's BehaviorSpace dialog.</p>
      <h3>Variables</h3>
      <table class="grid-table">
        <thead><tr><th>Variable</th><th>Values</th><th>Count</th></tr></thead>
        <tbody>
          {e.variables.map(([n, vals]) => (
            <tr><td class="mono">{n}</td><td class="mono">{vals.length > 12 ? `${vals.slice(0, 12).join(" ")} …` : vals.join(" ")}</td><td>{vals.length}</td></tr>
          ))}
        </tbody>
      </table>
      <div class="form-grid" style="margin-top: 12px">
        <label>Repetitions</label><div>{e.repetitions} ({e.sequentialRunOrder ? "sequential" : "interleaved"} run order)</div>
        <label>Runs</label><div><strong>{e.runCount}</strong></div>
        {row("Setup commands", e.setup)}
        {row("Go commands", e.go)}
        {row("Stop condition", e.exitCondition)}
        <label>Time limit</label><div>{e.timeLimit ? `${e.timeLimit} steps` : "none"}</div>
        <label>Measure runs</label><div>{e.runMetricsEveryStep ? "at every step" : "at the end of each run"}</div>
      </div>
      <h3>Metrics</h3>
      <ul class="findings mono">{e.metrics.map((x) => <li>{x}</li>)}</ul>
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
