// Interface editing (design 4.3): select, move, resize, add and delete widgets, a property
// sheet with NetLogo's fields, and undo and redo. Every change is a span edit in the host
// (D-19), so everything else in the file stays as it was.

import { computed, signal } from "@preact/signals";
import { useEffect, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import type { Widget } from "./ipc";
import { errorText } from "./ipc";
import { Icon } from "./icons";
import { type MenuItem, showContextMenu } from "./menu";
import { snap } from "./snap";
import { refreshExperiments } from "./experiments";
import { addActivity, dirty, history, model, notApplied, rightOpen, showToast } from "./state";
import { replaceOpenCode } from "./code";

export const editMode = signal(false);
export const selected = signal<number | null>(null);
export const canUndo = computed(() => history.value.canUndo);
export const canRedo = computed(() => history.value.canRedo);

export interface WidgetEdit {
  widgets: Widget[]; dirty: boolean; globalsChanged: boolean; canUndo: boolean; canRedo: boolean; code: string; info: string;
}

export type Op =
  | { op: "bounds"; index: number; x: number; y: number; width: number; height: number }
  | { op: "attr"; index: number; name: string; value: string }
  | { op: "text"; index: number; text: string }
  | { op: "add"; kind: string; x: number; y: number; name: string }
  | { op: "delete"; index: number }
  | { op: "world"; index: number; world: World }
  | { op: "part"; index: number; part: string; text: string }
  | { op: "batch"; ops: Op[] }
  | { op: "code"; text: string }
  | { op: "info"; text: string };

export interface World { minPxcor: number; maxPxcor: number; minPycor: number; maxPycor: number; patchSize: number }

export function applyResult(r: WidgetEdit) {
  const m = model.value;
  if (!m) return;
  model.value = { ...m, widgets: r.widgets, code: r.code, info: r.info };
  // The code editor follows when an undo or a replace changed the code.
  if (r.code !== m.code) { replaceOpenCode(r.code); notApplied.value = true; }
  dirty.value = r.dirty;
  history.value = { canUndo: r.canUndo, canRedo: r.canRedo };
  if (r.globalsChanged) {
    notApplied.value = true;
    showToast("The widgets' variables changed. Apply (on the Interface toolbar or in the Code tab) to use them in the model.");
  }
}

export async function editWidget(op: Op, what: string): Promise<boolean> {
  const m = model.value;
  if (!m) return false;
  try {
    applyResult(await invoke<WidgetEdit>("model_edit_widget", { id: m.id, op }));
    addActivity(what);
    return true;
  } catch (e) {
    showToast(`Can't change the widget: ${errorText(e)}`);
    return false;
  }
}

/** Undoes or redoes the last interface or experiment change (they share one history). */
export async function undoInterface(redo: boolean) {
  const m = model.value;
  if (!m) return;
  try {
    applyResult(await invoke<WidgetEdit>("model_undo", { id: m.id, redo }));
    void refreshExperiments();
  } catch (e) { showToast(errorText(e)); }
}

const KINDS: [string, string][] = [
  ["button", "Button"], ["slider", "Slider"], ["switch", "Switch"], ["chooser", "Chooser"],
  ["input", "Input box"], ["monitor", "Monitor"], ["plot", "Plot"], ["output", "Output"], ["note", "Note"],
];

/** A name not used by another widget, for a new widget's variable. */
function freshName(kind: string): string {
  const used = new Set((model.value?.widgets ?? []).map((w) => ("variable" in w.data ? w.data.variable : "")));
  const base = { slider: "new-slider", switch: "new-switch?", chooser: "new-choice", input: "new-input" }[kind];
  if (!base) return kind === "button" ? "setup" : kind === "monitor" ? "ticks" : kind === "note" ? "Note" : kind === "plot" ? "plot" : "";
  let name = base, i = 2;
  while (used.has(name)) name = base.endsWith("?") ? `${base.slice(0, -1)}-${i++}?` : `${base}-${i++}`;
  return name;
}

/** A widget kind chosen from Add, waiting for a click on the interface to place it. */
export const placing = signal<string | null>(null);
/** Lines a Shift-drag snapped to, drawn while dragging. */
export const guides = signal<{ x: number[]; y: number[] }>({ x: [], y: [] });

/** Adds a widget of `kind` with its top left corner at (x, y), selected for editing. */
export async function addAt(kind: string, x: number, y: number) {
  placing.value = null;
  editMode.value = true;
  const ok = await editWidget({ op: "add", kind, x: Math.max(0, Math.round(x)), y: Math.max(0, Math.round(y)), name: freshName(kind) }, `Added a ${kind}`);
  if (!ok) return;
  const n = model.value?.widgets.length ?? 0;
  selected.value = n - 1;
  rightOpen.value = true;
}

/** Starts placing a widget: the next click on the interface puts it there. */
export function startPlacing(kind: string) {
  editMode.value = true;
  placing.value = kind;
  const onKey = (e: KeyboardEvent) => {
    if (e.key !== "Escape") return;
    window.removeEventListener("keydown", onKey, true);
    if (placing.value) { placing.value = null; showToast("Not added."); }
  };
  window.addEventListener("keydown", onKey, true);
  const label = KINDS.find(([k]) => k === kind)?.[1].toLowerCase() ?? kind;
  showToast(`Click where the ${label} goes. Esc cancels.`);
}

/** The interface's right-click menu: add at the spot, or edit and delete the widget there. */
export function interfaceMenu(e: MouseEvent, x: number, y: number, w: Widget | undefined) {
  const items: MenuItem[] = [];
  if (w) {
    items.push(
      { label: `Edit ${w.element}…`, run: () => { editMode.value = true; selected.value = w.index; rightOpen.value = true; } },
      { label: `Delete ${w.element}`, disabled: w.data.kind === "view", run: () => { selected.value = w.index; void deleteSelected(); } },
      "separator",
    );
  }
  for (const [k, label] of KINDS) items.push({ label: `Add ${label.toLowerCase()} here`, run: () => void addAt(k, x, y) });
  if (editMode.value) items.push("separator", { label: "Done editing", run: () => { editMode.value = false; selected.value = null; } });
  showContextMenu(e, w ? `${w.element} menu` : "Interface menu", items);
}

export function AddMenu() {
  const [open, setOpen] = useState(false);
  const add = (kind: string) => {
    setOpen(false);
    if (!model.value) return;
    startPlacing(kind);
  };
  return (
    <div class="menu">
      <button class="btn small" aria-haspopup="true" aria-expanded={open} onClick={() => setOpen(!open)}>
        <Icon name="add" /> Add
      </button>
      {open && (
        <ul class="menu-list" role="menu" aria-label="Add widget">
          {KINDS.map(([k, label]) => (
            <li role="none"><button role="menuitem" onClick={() => add(k)}>{label}</button></li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** Handles over a widget in edit mode: drag to move, the corner to resize, keys too. */
export function EditFrame({ w }: { w: Widget }) {
  const isSel = selected.value === w.index;
  const [drag, setDrag] = useState<{ dx: number; dy: number; dw: number; dh: number } | null>(null);
  const start = (e: PointerEvent, resize: boolean) => {
    e.preventDefault();
    e.stopPropagation();
    selected.value = w.index;
    rightOpen.value = true;
    const sx = e.clientX, sy = e.clientY;
    const target = e.currentTarget as HTMLElement;
    // preventDefault stops the browser from focusing the frame; focus it so arrow keys work.
    (target.closest(".edit-frame") as HTMLElement | null)?.focus({ preventScroll: true });
    target.setPointerCapture(e.pointerId);
    let last = { dx: 0, dy: 0, dw: 0, dh: 0 };
    const others = (model.value?.widgets ?? []).filter((o) => o.index !== w.index);
    const move = (ev: PointerEvent) => {
      const dx = Math.round(ev.clientX - sx), dy = Math.round(ev.clientY - sy);
      last = resize ? { dx: 0, dy: 0, dw: dx, dh: dy } : { dx, dy, dw: 0, dh: 0 };
      if (ev.shiftKey) {
        // Shift: jump into line with the nearest edges of the other widgets.
        const r = { x: w.x + last.dx, y: w.y + last.dy, width: w.width + last.dw, height: w.height + last.dh };
        const sn = snap(r, others, resize);
        last = { dx: sn.x - w.x, dy: sn.y - w.y, dw: sn.width - w.width, dh: sn.height - w.height };
        guides.value = { x: sn.guidesX, y: sn.guidesY };
      } else if (guides.value.x.length || guides.value.y.length) {
        guides.value = { x: [], y: [] };
      }
      setDrag(last);
    };
    const up = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
      guides.value = { x: [], y: [] };
      setDrag(null);
      if (last.dx || last.dy || last.dw || last.dh) {
        editWidget({
          op: "bounds", index: w.index, x: Math.max(0, w.x + last.dx), y: Math.max(0, w.y + last.dy),
          width: Math.max(10, w.width + last.dw), height: Math.max(10, w.height + last.dh),
        }, resize ? "Resized a widget" : "Moved a widget");
      }
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  };
  const d = drag ?? { dx: 0, dy: 0, dw: 0, dh: 0 };
  const onKey = (e: KeyboardEvent) => {
    const step = e.shiftKey ? 10 : 1;
    const moves: Record<string, [number, number]> = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] };
    if (moves[e.key]) {
      e.preventDefault();
      const [mx, my] = moves[e.key];
      if (e.altKey) editWidget({ op: "bounds", index: w.index, x: w.x, y: w.y, width: Math.max(10, w.width + mx), height: Math.max(10, w.height + my) }, "Resized a widget");
      else editWidget({ op: "bounds", index: w.index, x: Math.max(0, w.x + mx), y: Math.max(0, w.y + my), width: w.width, height: w.height }, "Moved a widget");
    }
    if (e.key === "Delete" || e.key === "Backspace") { e.preventDefault(); deleteSelected(); }
    if (e.key === "Escape") selected.value = null;
  };
  return (
    <div class={`edit-frame${isSel ? " selected" : ""}`} tabIndex={0} role="button"
      aria-label={`${w.element} ${w.display ?? ""}, at ${w.x}, ${w.y}. Arrow keys move it, Alt+arrows resize, Delete removes.`}
      style={{ left: `${w.x + d.dx}px`, top: `${w.y + d.dy}px`, width: `${w.width + d.dw}px`, height: `${w.height + d.dh}px` }}
      onPointerDown={(e) => start(e as unknown as PointerEvent, false)}
      onFocus={() => { selected.value = w.index; }}
      onKeyDown={onKey}>
      {isSel && <span class="resize" onPointerDown={(e) => start(e as unknown as PointerEvent, true)} aria-hidden="true" />}
    </div>
  );
}

export async function deleteSelected() {
  const i = selected.value;
  const m = model.value;
  if (i === null || !m) return;
  const w = m.widgets[i];
  if (!w) return;
  if (w.data.kind === "view") { showToast("A model keeps its view."); return; }
  selected.value = null;
  await editWidget({ op: "delete", index: i }, `Deleted a ${w.element}`);
}

// ---- the property sheet ----------------------------------------------------------------

type Field = { name: string; label: string; kind?: "text" | "number" | "bool" | "area" | "select"; options?: string[]; text?: boolean; float?: boolean };

function fieldsFor(w: Widget): Field[] {
  const common: Field[] = [{ name: "display", label: "Label" }];
  switch (w.data.kind) {
    case "button": return [{ name: "source", label: "Commands", kind: "area", text: true }, ...common,
      { name: "forever", label: "Forever", kind: "bool" }, { name: "kind", label: "Agents", kind: "select", options: ["Observer", "Turtle", "Patch", "Link"] },
      { name: "actionKey", label: "Action key" }, { name: "disableUntilTicks", label: "Disabled until ticks start", kind: "bool" }];
    case "slider": return [{ name: "variable", label: "Global variable" }, ...common, { name: "min", label: "Minimum" }, { name: "step", label: "Increment" },
      { name: "max", label: "Maximum" }, { name: "default", label: "Value", kind: "number" }, { name: "units", label: "Units" },
      { name: "direction", label: "Direction", kind: "select", options: ["Horizontal", "Vertical"] }];
    case "switch": return [{ name: "variable", label: "Global variable" }, ...common, { name: "on", label: "On", kind: "bool" }];
    case "chooser": return [{ name: "variable", label: "Global variable" }, ...common];
    case "input": return [{ name: "variable", label: "Global variable" }, { name: "type", label: "Type", kind: "select", options: ["string", "number", "color", "reporter", "command"] },
      { name: "multiline", label: "Multi-line", kind: "bool" }];
    case "monitor": return [{ name: "source", label: "Reporter", kind: "area", text: true }, ...common,
      { name: "precision", label: "Decimal places", kind: "number" }, { name: "units", label: "Units" }, { name: "fontSize", label: "Font size", kind: "number" }];
    case "plot": return [...common, { name: "xAxis", label: "X axis label" }, { name: "yAxis", label: "Y axis label" },
      { name: "xMin", label: "X min", kind: "number", float: true }, { name: "xMax", label: "X max", kind: "number", float: true },
      { name: "yMin", label: "Y min", kind: "number", float: true }, { name: "yMax", label: "Y max", kind: "number", float: true },
      { name: "autoPlotX", label: "Auto scale X", kind: "bool" }, { name: "autoPlotY", label: "Auto scale Y", kind: "bool" }, { name: "legend", label: "Show legend", kind: "bool" }];
    case "note": return [{ name: "source", label: "Text", kind: "area", text: true }, { name: "fontSize", label: "Font size", kind: "number" }, { name: "markdown", label: "Markdown", kind: "bool" }];
    case "output": return [{ name: "fontSize", label: "Font size", kind: "number" }];
    case "view": return [{ name: "wrappingAllowedX", label: "Wrap horizontally", kind: "bool" },
      { name: "wrappingAllowedY", label: "Wrap vertically", kind: "bool" },
      { name: "showTickCounter", label: "Show tick counter", kind: "bool" }, { name: "tickCounterLabel", label: "Tick counter label" },
      { name: "fontSize", label: "Label font size", kind: "number" }, { name: "frameRate", label: "Frame rate", kind: "number", float: true }];
    default: return [];
  }
}

function currentValue(w: Widget, f: Field): string {
  const d = w.data as unknown as Record<string, unknown>;
  if (f.text) return String((d.source ?? d.text ?? "") as string);
  const map: Record<string, string> = { default: "value", kind: "agentKind", type: "inputType", wrappingAllowedX: "wrapX", wrappingAllowedY: "wrapY" };
  if (f.name === "display") return w.display ?? "";
  if (f.name === "direction") return d.vertical ? "Vertical" : "Horizontal";
  const v = d[map[f.name] ?? f.name];
  return v === null || v === undefined ? "" : String(v);
}

export function PropertySheet() {
  const m = model.value;
  const i = selected.value;
  const w = i !== null ? m?.widgets[i] : undefined;
  if (!w) return <p class="empty-note">Select a widget to see its properties. Arrow keys move it; Alt+arrows resize; Delete removes it; Ctrl+Z undoes.</p>;
  const fields = fieldsFor(w);
  return (
    <section class="inspector" aria-label={`Properties of the ${w.element}`}>
      <strong style="text-transform: capitalize">{w.element}</strong>
      {w.data.kind === "view" && <WorldSettings w={w} />}
      {w.data.kind === "chooser" && <p class="empty-note" style="padding:0">Editing the choices comes with the next editing step.</p>}
      <div class="form-grid" style="grid-template-columns: 110px 1fr">
        {fields.map((f) => <PropField w={w} f={f} />)}
        {(["x", "y", "width", "height"] as const).map((k) => (
          <PropField w={w} f={{ name: k, label: k === "x" ? "Left" : k === "y" ? "Top" : k === "width" ? "Width" : "Height", kind: "number" }} />
        ))}
      </div>
      <div class="actions">
        {w.data.kind !== "view" && <button class="btn small" onClick={deleteSelected}>Delete</button>}
        {w.oldSize && <span class="muted" style="font-size: 12px">Old-size widget (D-18)</span>}
      </div>
    </section>
  );
}

function PropField({ w, f }: { w: Widget; f: Field }) {
  const geometry = ["x", "y", "width", "height"].includes(f.name);
  const initial = geometry ? String((w as unknown as Record<string, number>)[f.name]) : currentValue(w, f);
  const [v, setV] = useState(initial);
  useEffect(() => setV(initial), [initial, w.index]);
  const id = `prop-${w.index}-${f.name}`;
  const commit = (value: string) => {
    if (value === initial) return;
    if (geometry) {
      const n = Number(value);
      if (!Number.isFinite(n)) return;
      const b = { x: w.x, y: w.y, width: w.width, height: w.height, [f.name]: Math.round(n) };
      editWidget({ op: "bounds", index: w.index, ...b }, `Changed ${f.label.toLowerCase()} of a ${w.element}`);
    } else if (f.text) {
      editWidget({ op: "text", index: w.index, text: value }, `Changed the ${f.label.toLowerCase()} of a ${w.element}`);
    } else {
      const val = (f.name === "default" || f.float) && /^-?\d+$/.test(value.trim()) ? `${value.trim()}.0` : value;
      editWidget({ op: "attr", index: w.index, name: f.name, value: val }, `Changed ${f.label.toLowerCase()} of a ${w.element}`);
    }
  };
  return (
    <>
      <label for={id}>{f.label}</label>
      {f.kind === "bool" ? (
        <input id={id} type="checkbox" checked={v === "true"} onChange={(e) => { const s = String((e.target as HTMLInputElement).checked); setV(s); commit(s); }} />
      ) : f.kind === "select" ? (
        <select id={id} value={v} onChange={(e) => { const s = (e.target as HTMLSelectElement).value; setV(s); commit(s); }}>
          {f.options!.map((o) => <option value={o}>{o}</option>)}
        </select>
      ) : f.kind === "area" ? (
        <textarea id={id} class="mono" rows={3} value={v} onInput={(e) => setV((e.target as HTMLTextAreaElement).value)} onBlur={() => commit(v)} />
      ) : (
        <input id={id} type="text" class={f.kind === "number" ? "mono" : undefined} value={v}
          onInput={(e) => setV((e.target as HTMLInputElement).value)} onBlur={() => commit(v)}
          onKeyDown={(e) => { if (e.key === "Enter") commit(v); }} />
      )}
    </>
  );
}

// ---- world settings ------------------------------------------------------------------

/** World size and patch size (NetLogo's Model Settings), on the view's property sheet.
 * They are written together, so the view's size follows them; the running world changes
 * on Apply. */
function WorldSettings({ w }: { w: Widget }) {
  if (w.data.kind !== "view") return null;
  const d = w.data;
  const initial = { minPxcor: d.minPxcor, maxPxcor: d.maxPxcor, minPycor: d.minPycor, maxPycor: d.maxPycor, patchSize: d.patchSize };
  const key = JSON.stringify(initial);
  const [draft, setDraft] = useState<Record<keyof World, string>>(() => strings(initial));
  useEffect(() => setDraft(strings(initial)), [key]);
  const nums = Object.fromEntries(Object.entries(draft).map(([k, v]) => [k, Number(v)])) as unknown as World;
  const problem = !Object.values(nums).every(Number.isFinite) ? "Every field needs a number."
    : !["minPxcor", "maxPxcor", "minPycor", "maxPycor"].every((k) => Number.isInteger(nums[k as keyof World])) ? "Coordinates are whole numbers."
    : nums.minPxcor > 0 || nums.maxPxcor < 0 || nums.minPycor > 0 || nums.maxPycor < 0 ? "The world must contain the origin (0, 0)."
    : nums.patchSize <= 0 ? "The patch size must be more than 0." : null;
  const changed = JSON.stringify(nums) !== key;
  const apply = async () => {
    if (problem || !changed) return;
    if (await editWidget({ op: "world", index: w.index, world: nums }, "Changed the world size")) {
      notApplied.value = true;
      showToast("The world size changes in the running model on Apply.");
    }
  };
  const field = (k: keyof World, label: string) => (
    <>
      <label for={`world-${k}`}>{label}</label>
      <input id={`world-${k}`} type="text" class="mono" value={draft[k]}
        onInput={(e) => setDraft({ ...draft, [k]: (e.target as HTMLInputElement).value })}
        onKeyDown={(e) => { if (e.key === "Enter") apply(); }} />
    </>
  );
  const width = Number.isFinite(nums.maxPxcor - nums.minPxcor) ? nums.maxPxcor - nums.minPxcor + 1 : 0;
  const height = Number.isFinite(nums.maxPycor - nums.minPycor) ? nums.maxPycor - nums.minPycor + 1 : 0;
  return (
    <fieldset class="world">
      <legend>World</legend>
      <div class="form-grid" style="grid-template-columns: 110px 1fr">
        {field("minPxcor", "min-pxcor")}
        {field("maxPxcor", "max-pxcor")}
        {field("minPycor", "min-pycor")}
        {field("maxPycor", "max-pycor")}
        {field("patchSize", "Patch size")}
      </div>
      <p class="empty-note" style="padding: 2px 0" role="status">
        {problem ?? `${width} × ${height} patches, view ${Math.round(width * nums.patchSize)} × ${Math.round(height * nums.patchSize)} pixels`}
      </p>
      <div class="actions">
        <button class="btn small" disabled={!!problem || !changed} onClick={apply}>Set world size</button>
        {changed && <button class="btn small" onClick={() => setDraft(strings(initial))}>Reset</button>}
      </div>
    </fieldset>
  );
}

function strings(w: World): Record<keyof World, string> {
  return Object.fromEntries(Object.entries(w).map(([k, v]) => [k, String(v)])) as Record<keyof World, string>;
}
