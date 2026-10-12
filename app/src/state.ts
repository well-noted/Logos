// The app's state, as Preact signals. Components read signals; actions below change them and
// call the host.

import { computed, signal } from "@preact/signals";
import { confirmDialog } from "./dialog";
import { checkRecovery } from "./recovery";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog, save as saveDialog, ask, message } from "@tauri-apps/plugin-dialog";
import {
  errorText, ipc, on, type EngineStatus, type ModelView, type OutlineEntry, type Settings, type WidgetValue,
} from "./ipc";
import { applyTheme, customTheme, parseCustomTheme, resolveTheme, type Theme } from "./themes";

// ---- engine and settings ------------------------------------------------------------------

export const engine = signal<EngineStatus>({ state: "searching", installs: [] });
export const settings = signal<Settings>({ recent: [], theme: "", fontSize: 0, codeFontSize: 0 });
export const customThemes = signal<Theme[]>([]);
export const appInfo = signal<{ version: string; dataDir: string; platform: string; netlogoVersion: string } | null>(null);

export const theme = computed(() => resolveTheme(settings.value.theme, customThemes.value));

export function refreshTheme() {
  applyTheme(theme.value, settings.value.fontSize, settings.value.codeFontSize,
    settings.value.fontFamily as string | undefined, settings.value.codeFontFamily as string | undefined);
}

export async function saveSettings(patch: Partial<Settings>) {
  settings.value = { ...settings.value, ...patch };
  refreshTheme();
  await ipc.settingsSet(settings.value);
}

export async function loadCustomThemes() {
  const list = await ipc.themesList();
  customThemes.value = list
    .map((t) => parseCustomTheme(t.json))
    .filter((f): f is NonNullable<typeof f> => f !== null)
    .map(customTheme);
  refreshTheme();
}

// ---- the open model -----------------------------------------------------------------------

export interface LogLine { time: number; level: "info" | "warning" | "error"; text: string }
export interface ActivityEntry { time: number; actor: string; text: string }

export const model = signal<ModelView | null>(null);
export const dirty = signal(false);
export const notApplied = signal(false);
/** Whether the model's undo history (code, Info, interface, experiments) has steps. */
export const history = signal({ canUndo: false, canRedo: false });
export const running = signal<{ button: number | null } | null>(null);
export const ticks = signal<number | null>(null);
export const monitors = signal<Record<number, { text: string; error: boolean }>>({});
/** What the model printed to the Output widget (output-print and the like). */
export const output = signal<string>("");
/** The Command Center's log: commands typed, what print and show wrote, and errors. */
export interface CcLine { kind: "input" | "output" | "error"; text: string; agents?: string }
export const ccLines = signal<CcLine[]>([]);
function addCc(line: CcLine) {
  const lines = ccLines.value;
  const last = lines[lines.length - 1];
  // Consecutive printed text joins one block.
  if (line.kind === "output" && last?.kind === "output") {
    ccLines.value = [...lines.slice(0, -1), { ...last, text: last.text + line.text }];
  } else {
    ccLines.value = [...lines, line].slice(-2000);
  }
}
export interface Problem { text: string; source: string; span?: [number, number] }
export const problems = signal<Problem[]>([]);
export const log = signal<LogLine[]>([]);
export const activity = signal<ActivityEntry[]>([]);
export const outline = signal<OutlineEntry[]>([]);
export const speed = signal(100);
export const viewOnTicks = signal(true);
export const lastFrameTicks = signal<number | null>(null);

export type Inspected = { kind: "turtle"; who: number } | { kind: "patch"; x: number; y: number };
export const inspected = signal<Inspected | null>(null);
export interface Watch { reporter: string; values: number[]; text: string; ok: boolean }
export const watches = signal<Watch[]>([]);
export const pauseWhen = signal<string>("");

export function inspect(target: Inspected) {
  inspected.value = target;
  rightOpen.value = true;
}

export type ModelTab = "interface" | "info" | "code";

// ---- documents in the editor area (design 2.3) ---------------------------------------------

export type Doc =
  | { id: "model"; kind: "model" }
  | { id: "project"; kind: "project" }
  | { id: "secrets"; kind: "secrets" }
  | { id: string; kind: "experiment"; name: string }
  | { id: string; kind: "data"; path: string }
  | { id: string; kind: "diff"; label: string; code: string }
  | { id: string; kind: "dashboard"; runId: number; name: string; dir?: string };

export const docs = signal<Doc[]>([]);
export const activeDoc = signal<string>("model");
/** A second document shown beside the active one (one split, design 2.2). */
export const sideDoc = signal<string | null>(null);

export function openDoc(doc: Doc) {
  if (!docs.value.some((d) => d.id === doc.id)) docs.value = [...docs.value, doc];
  activeDoc.value = doc.id;
}

export function closeDoc(id: string) {
  if (id === "model") return;
  docs.value = docs.value.filter((d) => d.id !== id);
  if (activeDoc.value === id) activeDoc.value = "model";
  if (sideDoc.value === id) sideDoc.value = null;
}

// ---- project ------------------------------------------------------------------------------

export interface ProjectView {
  dir: string; isProject: boolean; model: string; engine?: string | null;
  files: { path: string; role: string; description?: string | null; dictionary?: string | null; exists: boolean }[];
  required: string[]; optional: string[]; requirements?: string | null; interpreter?: string | null; source?: string | null;
}
export interface Finding { severity: "ok" | "warning" | "error"; subject: string; message: string }
export const project = signal<ProjectView | null>(null);
export const findings = signal<Finding[]>([]);

export async function refreshProject() {
  const m = model.value;
  if (!m) { project.value = null; findings.value = []; return; }
  try {
    project.value = await invoke<ProjectView>("project_status", { id: m.id });
    findings.value = project.value.isProject ? await invoke<Finding[]>("project_check", { id: m.id }) : [];
    const checkProblems = findings.value
      .filter((f) => f.severity !== "ok")
      .map((f) => ({ text: `${f.subject}: ${f.message}`, source: f.severity === "error" ? "check" : "check (warning)" }));
    problems.value = [...problems.value.filter((p) => !p.source.startsWith("check")), ...checkProblems];
  } catch (e) {
    addLog("warning", `Project: ${errorText(e)}`);
  }
}
export const modelTab = signal<ModelTab>("interface");

// ---- layout -------------------------------------------------------------------------------

export type Activity = "model" | "files" | "search" | "experiments" | "results" | "llm" | "git";
export const activityView = signal<Activity | null>("model");
export type BottomTab = "command" | "output" | "problems" | "log" | "activity";
export const bottomTab = signal<BottomTab>("command");
export const bottomOpen = signal(true);
export const rightOpen = signal(false);
export const viewPopped = signal(false);
export const paletteOpen = signal(false);
export const settingsOpen = signal(false);
export const toast = signal<string | null>(null);

export function addLog(level: LogLine["level"], text: string) {
  if (level !== "info") invoke("ui_log", { level, text }).catch(() => {});
  log.value = [...log.value.slice(-499), { time: Date.now(), level, text }];
}

export function addActivity(text: string, actor = "user") {
  activity.value = [...activity.value.slice(-499), { time: Date.now(), actor, text }];
}

export function showToast(text: string) {
  toast.value = text;
  setTimeout(() => { if (toast.value === text) toast.value = null; }, 4000);
}

function report(e: unknown, what: string) {
  const text = `${what}: ${errorText(e)}`;
  addLog("error", text);
  showToast(text);
}

// ---- actions ------------------------------------------------------------------------------

export async function init() {
  appInfo.value = await ipc.appInfo();
  settings.value = await ipc.settingsGet();
  await loadCustomThemes();
  window.matchMedia?.("(prefers-color-scheme: dark)").addEventListener?.("change", refreshTheme);
  engine.value = await ipc.engineStatus();
  on("engine", (s) => { engine.value = s; addLog(s.state === "failed" ? "error" : "info", `Engine: ${s.state}${s.error ? ` (${s.error})` : ""}`); });
  on("model-output", (e) => {
    if (e.model !== model.value?.id) return;
    if (e.outputArea) output.value = (output.value + e.text).slice(-200_000);
    else addCc({ kind: "output", text: e.text });
  });
  on("model-monitors", (e) => {
    if (e.model !== model.value?.id) return;
    const next: Record<number, { text: string; error: boolean }> = {};
    for (const v of e.values) next[v.index] = { text: v.text, error: v.error };
    monitors.value = next;
    ticks.value = e.ticks;
  });
  on("model-ticks", (e) => { if (e.model === model.value?.id) ticks.value = e.ticks; });
  on("model-run", (e) => {
    if (e.model !== model.value?.id) return;
    running.value = e.running ? { button: e.button ?? null } : null;
    if (!e.running) {
      if (e.error) {
        const text = `Runtime error: ${e.error.message}${e.error.detail ? ` (${e.error.detail})` : ""}`;
        problems.value = [...problems.value, { text, source: "run" }];
        addLog("error", text);
        showToast(text);
      }
    }
  });
  on("model-widgets", async (e) => {
    const m = model.value;
    if (!m || e.model !== m.id) return;
    const widgets = await ipc.widgets(m.id);
    model.value = { ...m, widgets };
    dirty.value = true;
    addActivity("Rename changed widgets or experiments");
  });
  on("model-error", (e) => { if (e.model === model.value?.id) report(e.error, "Command"); });
  const first = await ipc.startupModel();
  if (first) {
    const tryOpen = () => {
      if (engine.value.state === "ready") { void openModel(first); return true; }
      return engine.value.state === "failed" || engine.value.state === "notFound";
    };
    if (!tryOpen()) {
      const timer = setInterval(() => { if (tryOpen()) clearInterval(timer); }, 300);
    }
  }
}

export async function openModelDialog(defaultPath?: string) {
  const path = await openDialog({
    multiple: false, directory: false, defaultPath,
    filters: [{ name: "NetLogo models", extensions: ["nlogox", "nlogo"] }],
  });
  if (typeof path === "string") await openModel(path);
}

export async function openModel(path: string) {
  if (model.value) {
    if (!(await closeModel())) return;
  }
  try {
    const view = await ipc.open(path);
    loadView(view);
    void checkRecovery(view.id, view.name);
    settings.value = await ipc.settingsGet();
    addActivity(`Opened ${view.name}`);
  } catch (e) {
    const err = e as { message?: string; detail?: string };
    if (err?.message === "legacy" && err.detail) {
      const ok = await ask(
        `${path} is a model saved before NetLogo 7. Logos converts a copy to the .nlogox format through NetLogo and leaves the original unchanged.\n\nThe converted copy will be saved as:\n${err.detail}`,
        { title: "Convert this model?", kind: "info", okLabel: "Convert", cancelLabel: "Cancel" },
      );
      if (!ok) return;
      try {
        const r = await ipc.convert(path, err.detail);
        problems.value = r.warnings.map((w) => ({ text: `Conversion: ${w}`, source: "convert" }));
        addActivity(`Converted ${path} (${r.sourceVersion}) to ${r.outputPath}`);
        await openModel(r.outputPath);
        if (r.warnings.length) bottomTab.value = "problems";
      } catch (ce) {
        report(ce, "Conversion failed");
      }
      return;
    }
    report(e, "Can't open the model");
  }
}

function loadView(view: ModelView) {
  model.value = view;
  dirty.value = view.dirty;
  notApplied.value = false;
  running.value = null;
  monitors.value = {};
  output.value = "";
  ccLines.value = [];
  ticks.value = null;
  problems.value = view.errors.map((e) => ({ text: e.message, source: e.detail || "code", span: e.detail ? undefined : e.span }));
  if (!view.compiled) {
    modelTab.value = "code";
    bottomTab.value = "problems";
  }
  refreshOutline();
  docs.value = [{ id: "model", kind: "model" }];
  activeDoc.value = "model";
  refreshProject();
  document.title = `${view.name} - Logos`;
}

export async function refreshOutline() {
  const m = model.value;
  if (!m) return;
  try { outline.value = await ipc.outline(m.id); } catch { outline.value = []; }
}

export async function closeModel(): Promise<boolean> {
  const m = model.value;
  if (!m) return true;
  if (dirty.value) {
    const save = await ask(`Save changes to ${m.name}?`, {
      title: "Unsaved changes", kind: "warning", okLabel: "Save", cancelLabel: "Don't save",
    });
    if (save) await saveModel();
  }
  try { await ipc.close(m.id); } catch { /* the engine may be gone */ }
  model.value = null;
  running.value = null;
  docs.value = [];
  project.value = null;
  document.title = "Logos";
  return true;
}

export async function saveModel() {
  const m = model.value;
  if (!m) return;
  try {
    await ipc.save(m.id);
    dirty.value = false;
    addActivity(`Saved ${m.name}`);
    showToast(`Saved ${m.name}`);
  } catch (e) {
    report(e, "Can't save");
  }
}

export async function pressButton(index: number) {
  const m = model.value;
  if (!m) return;
  const w = m.widgets[index];
  try {
    await ipc.press(m.id, index);
    if (w?.data.kind === "button") addActivity(`Pressed ${w.display || w.data.source.trim().split("\n")[0]}`);
  } catch (e) {
    report(e, "Button");
  }
}

export async function setWidgetValue(index: number, value: WidgetValue) {
  const m = model.value;
  if (!m) return;
  try {
    await ipc.setValue(m.id, index, value);
    dirty.value = true;
    const w = m.widgets[index];
    if (w && "variable" in w.data) {
      const shown = value.kind === "choice" && w.data.kind === "chooser" ? w.data.choices[value.value]?.label : value.value;
      addActivity(`Set ${w.data.variable} to ${shown}`);
    }
    // Keep the widget model in step, so redraws show the new value.
    const widgets = m.widgets.slice();
    const d = widgets[index].data;
    if (d.kind === "slider" && value.kind === "number") widgets[index] = { ...widgets[index], data: { ...d, value: value.value } };
    if (d.kind === "switch" && value.kind === "bool") widgets[index] = { ...widgets[index], data: { ...d, on: value.value } };
    if (d.kind === "chooser" && value.kind === "choice") widgets[index] = { ...widgets[index], data: { ...d, current: value.value } };
    if (d.kind === "input" && value.kind === "text") widgets[index] = { ...widgets[index], data: { ...d, value: value.value } };
    model.value = { ...m, widgets };
  } catch (e) {
    report(e, "Can't set the value");
  }
}

export async function runCommand(source: string, agents: string) {
  const m = model.value;
  if (!m || !source.trim()) return;
  addCc({ kind: "input", text: source, agents });
  try {
    await ipc.command(m.id, source, agents);
    addActivity(`Ran ${agents === "observer" ? "" : agents + " "}command: ${source}`);
  } catch (e) {
    const t = errorText(e);
    addCc({ kind: "error", text: t });
    addLog("error", t);
  }
}

export async function stepModel() {
  const m = model.value;
  if (!m) return;
  try { await ipc.step(m.id); } catch (e) { report(e, "Step"); }
}

export async function stopModel() {
  const m = model.value;
  if (!m) return;
  try { await ipc.stop(m.id); } catch (e) { report(e, "Stop"); }
}

export async function setSpeed(v: number) {
  speed.value = v;
  const m = model.value;
  if (m) await ipc.speed(m.id, v).catch((e) => report(e, "Speed"));
}

export async function setCode(code: string) {
  const m = model.value;
  if (!m) return;
  model.value = { ...m, code };
  const r = await ipc.setCode(m.id, code);
  dirty.value = r.dirty;
  notApplied.value = r.notApplied;
  history.value = { canUndo: r.canUndo, canRedo: r.canRedo };
}

export async function setInfo(info: string) {
  const m = model.value;
  if (!m) return;
  model.value = { ...m, info };
  const r = await ipc.setInfo(m.id, info);
  dirty.value = r.dirty;
  history.value = { canUndo: r.canUndo, canRedo: r.canRedo };
}

/** Apply (D-23). Until the bridge can recompile in place (G5), Logos saves the model and opens
 * it again, which clears the world, so it asks first. */
export async function applyCode() {
  const m = model.value;
  if (!m) return;
  if (running.value) {
    showToast("Pause the model before applying code changes.");
    return;
  }
  if (settings.value.confirmApply !== false) {
    const r = await confirmDialog({
      title: "Apply code changes",
      message: "Applying saves the model and reloads it in NetLogo. The world is cleared, as when NetLogo opens a model. (Applying without clearing the world comes with the bridge's recompile method, G5.)",
      okLabel: "Save and apply", dontAsk: true,
    });
    if (!r.ok) return;
    if (r.dontAsk) await saveSettings({ confirmApply: false });
  }
  try {
    const view = await ipc.apply(m.id);
    model.value = { ...view };
    dirty.value = false;
    notApplied.value = false;
    problems.value = view.errors.map((e) => ({ text: e.message, source: e.detail || "code", span: e.detail ? undefined : e.span }));
    addActivity("Applied code changes");
    showToast(view.compiled ? "Code applied" : "The code has errors; see Problems");
    if (!view.compiled) bottomTab.value = "problems";
    refreshOutline();
  } catch (e) {
    report(e, "Apply failed");
  }
}

export async function chooseNetLogoFolder() {
  const path = await openDialog({ directory: true, multiple: false, title: "Choose the NetLogo 7.0.4 folder" });
  if (typeof path !== "string") return;
  try { engine.value = await ipc.engineSetHome(path); } catch (e) { await message(errorText(e), { kind: "error" }); }
}

export async function exportThemeFile(json: string, name: string) {
  return saveDialog({ defaultPath: `${name}.logos-theme.json`, filters: [{ name: "Logos theme", extensions: ["json"] }] })
    .then((p) => ({ path: p, json }));
}
