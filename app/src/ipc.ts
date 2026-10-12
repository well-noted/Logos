// Calls into the Tauri host (crates/logos-app) and its events. Every command the host offers
// is wrapped here with its types, so the rest of the front end never names a command string.

import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface Install { path: string; version: string; source: string }

export interface EngineStatus {
  state: "searching" | "notFound" | "starting" | "ready" | "failed";
  installs: Install[];
  home?: string | null;
  version?: string | null;
  hello?: {
    netlogoVersion?: string | null; javaVersion: string; javaVendor: string;
    bridgeVersion: string; extensionsDirs: string[]; modelsDir?: string | null;
  } | null;
  error?: string | null;
  log?: string | null;
}

export interface Settings {
  netlogoHome?: string | null;
  recent: string[];
  theme: string;
  fontSize: number;
  codeFontSize: number;
  [other: string]: unknown;
}

export interface AppError { message: string; span?: [number, number]; detail?: string }

export type AgentKind = "Observer" | "Turtle" | "Patch" | "Link";

export interface Pen {
  display: string; interval: number; mode: number; color: number; legend: boolean;
  setup: string; update: string;
}
export interface Choice { kind: string; code: string; label: string }

export type WidgetData =
  | { kind: "view"; minPxcor: number; maxPxcor: number; minPycor: number; maxPycor: number;
      patchSize: number; wrapX: boolean; wrapY: boolean; fontSize: number; updateMode: number;
      showTickCounter: boolean; tickCounterLabel?: string | null; frameRate: number }
  | { kind: "button"; source: string; forever: boolean; agentKind: AgentKind;
      actionKey?: string | null; disableUntilTicks: boolean }
  | { kind: "slider"; variable: string; min: string; max: string; step: string; value: number;
      units?: string | null; vertical: boolean }
  | { kind: "switch"; variable: string; on: boolean }
  | { kind: "chooser"; variable: string; choices: Choice[]; current: number }
  | { kind: "input"; variable: string; inputType: string; value: string; multiline: boolean }
  | { kind: "monitor"; source: string; precision: number; fontSize: number; units?: string | null }
  | { kind: "plot"; xAxis?: string | null; yAxis?: string | null; xMin: number; xMax: number;
      yMin: number; yMax: number; autoPlotX: boolean; autoPlotY: boolean; legend: boolean;
      setup: string; update: string; pens: Pen[] }
  | { kind: "output"; fontSize: number }
  | { kind: "note"; text: string; fontSize: number; markdown: boolean;
      textColorLight?: number | null; textColorDark?: number | null;
      backgroundLight?: number | null; backgroundDark?: number | null }
  | { kind: "unknown" };

export interface Widget {
  index: number; element: string; x: number; y: number; width: number; height: number;
  oldSize: boolean; display?: string | null; data: WidgetData; problems: string[];
}

export interface ModelView {
  id: number; path: string; name: string; version?: string | null; widgets: Widget[];
  code: string; info: string; compiled: boolean; errors: AppError[]; dirty: boolean;
}

export interface OutlineEntry {
  name: string; kind: string; detail?: string | null; start: number; end: number; line: number;
}

export type WidgetValue =
  | { kind: "number"; value: number }
  | { kind: "bool"; value: boolean }
  | { kind: "choice"; value: number }
  | { kind: "text"; value: string };

export const ipc = {
  appInfo: () => invoke<{ version: string; dataDir: string; platform: string; netlogoVersion: string }>("app_info"),
  startupModel: () => invoke<string | null>("startup_model"),
  engineStatus: () => invoke<EngineStatus>("engine_status"),
  engineSetHome: (path: string) => invoke<EngineStatus>("engine_set_home", { path }),
  engineRestart: () => invoke<EngineStatus>("engine_restart"),
  settingsGet: () => invoke<Settings>("settings_get"),
  settingsSet: (settings: Settings) => invoke<void>("settings_set", { settings }),
  themesList: () => invoke<{ name: string; json: string }[]>("themes_list"),
  themeSave: (name: string, json: string) => invoke<void>("theme_save", { name, json }),
  themeDelete: (name: string) => invoke<void>("theme_delete", { name }),
  open: (path: string) => invoke<ModelView>("model_open", { path }),
  convert: (source: string, output: string) =>
    invoke<{ outputPath: string; sourceVersion: string; warnings: string[] }>("model_convert", { source, output }),
  attachView: (id: number, onFrame: (buf: ArrayBuffer) => void) => {
    const channel = new Channel<ArrayBuffer>();
    channel.onmessage = onFrame;
    return invoke<void>("model_attach_view", { id, channel });
  },
  close: (id: number) => invoke<void>("model_close", { id }),
  command: (id: number, source: string, agents: string) => invoke<void>("model_command", { id, source, agents }),
  press: (id: number, index: number) => invoke<void>("model_press", { id, index }),
  step: (id: number) => invoke<void>("model_step", { id }),
  stop: (id: number) => invoke<void>("model_stop", { id }),
  setValue: (id: number, index: number, value: WidgetValue) => invoke<void>("model_set_value", { id, index, value }),
  speed: (id: number, speed: number) => invoke<void>("model_speed", { id, speed }),
  viewUpdates: (id: number, onTicks: boolean) => invoke<void>("model_view_updates", { id, onTicks }),
  observe: (id: number, reporters: string[]) =>
    invoke<{ ticks: number | null; values: { text: string; ok: boolean }[] }>("model_observe", { id, reporters }),
  setCode: (id: number, code: string) => invoke<{ dirty: boolean; notApplied: boolean }>("model_set_code", { id, code }),
  setInfo: (id: number, info: string) => invoke<boolean>("model_set_info", { id, info }),
  save: (id: number) => invoke<void>("model_save", { id }),
  apply: (id: number) => invoke<ModelView>("model_apply", { id }),
  outline: (id: number) => invoke<OutlineEntry[]>("model_outline", { id }),
};

export interface Events {
  engine: EngineStatus;
  "model-output": { model: number; text: string };
  "model-monitors": { model: number; ticks: number | null; values: { index: number; text: string; error: boolean }[] };
  "model-ticks": { model: number; ticks: number | null };
  "model-run": { model: number; running: boolean; button?: number | null; reason?: string | null; error?: AppError | null };
  "model-error": { model: number; error: AppError };
}

export function on<K extends keyof Events>(name: K, f: (payload: Events[K]) => void): Promise<UnlistenFn> {
  return listen<Events[K]>(name, (e) => f(e.payload));
}

/** The message of an error a command rejected with. */
export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) {
    const a = e as AppError;
    return a.detail && a.message !== "legacy" ? `${a.message} (${a.detail})` : a.message;
  }
  return String(e);
}
