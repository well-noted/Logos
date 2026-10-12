// Interface editing (design 4.3): select, move, resize, add and delete widgets, a property
// sheet with NetLogo's fields, and undo and redo. Every change is a span edit in the host
// (D-19), so everything else in the file stays as it was.

import { signal } from "@preact/signals";
import { useEffect, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import type { Widget } from "./ipc";
import { errorText } from "./ipc";
import { Icon } from "./icons";
import { addActivity, dirty, model, notApplied, rightOpen, showToast } from "./state";

export const editMode = signal(false);
export const selected = signal<number | null>(null);
export const canUndo = signal(false);
export const canRedo = signal(false);

interface WidgetEdit { widgets: Widget[]; dirty: boolean; globalsChanged: boolean; canUndo: boolean; canRedo: boolean }

type Op =
  | { op: "bounds"; index: number; x: number; y: number; width: number; height: number }
  | { op: "attr"; index: number; name: string; value: string }
  | { op: "text"; index: number; text: string }
  | { op: "add"; kind: string; x: number; y: number; name: string }
  | { op: "delete"; index: number };

function applyResult(r: WidgetEdit) {
  const m = model.value;
  if (!m) return;
  model.value = { ...m, widgets: r.widgets };
  dirty.value = r.dirty;
  canUndo.value = r.canUndo;
  canRedo.value = r.canRedo;
  if (r.globalsChanged) {
    notApplied.value = true;
    showToast("The widgets' variables changed. Apply (in the Code tab) to use them in the model.");
  }
}

export async function editWidget(op: Op, what: string) {
  const m = model.value;
  if (!m) return;
  try {
    applyResult(await invoke<WidgetEdit>("model_edit_widget", { id: m.id, op }));
    addActivity(what);
  } catch (e) {
    showToast(`Can't change the widget: ${errorText(e)}`);
  }
}

export async function undoInterface(redo: boolean) {
  const m = model.value;
  if (!m) return;
  try { applyResult(await invoke<WidgetEdit>("model_undo", { id: m.id, redo })); }
  catch (e) { showToast(errorText(e)); }
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

export function AddMenu() {
  const [open, setOpen] = useState(false);
  const add = async (kind: string) => {
    setOpen(false);
    const m = model.value;
    if (!m) return;
    editMode.value = true;
    const bottom = Math.max(10, ...m.widgets.map((w) => w.y + w.height + 10));
    await editWidget({ op: "add", kind, x: 10, y: bottom, name: freshName(kind) }, `Added a ${kind}`);
    const n = model.value?.widgets.length ?? 0;
    selected.value = n - 1;
    rightOpen.value = true;
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
    target.setPointerCapture(e.pointerId);
    let last = { dx: 0, dy: 0, dw: 0, dh: 0 };
    const move = (ev: PointerEvent) => {
      const dx = Math.round(ev.clientX - sx), dy = Math.round(ev.clientY - sy);
      last = resize ? { dx: 0, dy: 0, dw: dx, dh: dy } : { dx, dy, dw: 0, dh: 0 };
      setDrag(last);
    };
    const up = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
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

type Field = { name: string; label: string; kind?: "text" | "number" | "bool" | "area" | "select"; options?: string[]; text?: boolean };

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
      { name: "xMin", label: "X min", kind: "number" }, { name: "xMax", label: "X max", kind: "number" },
      { name: "yMin", label: "Y min", kind: "number" }, { name: "yMax", label: "Y max", kind: "number" },
      { name: "autoPlotX", label: "Auto scale X", kind: "bool" }, { name: "autoPlotY", label: "Auto scale Y", kind: "bool" }, { name: "legend", label: "Show legend", kind: "bool" }];
    case "note": return [{ name: "source", label: "Text", kind: "area", text: true }, { name: "fontSize", label: "Font size", kind: "number" }, { name: "markdown", label: "Markdown", kind: "bool" }];
    case "output": return [{ name: "fontSize", label: "Font size", kind: "number" }];
    case "view": return [];
    default: return [];
  }
}

function currentValue(w: Widget, f: Field): string {
  const d = w.data as unknown as Record<string, unknown>;
  if (f.text) return String((d.source ?? d.text ?? "") as string);
  const map: Record<string, string> = { default: "value", kind: "agentKind", type: "inputType" };
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
      {w.data.kind === "view" && <p class="empty-note" style="padding:0">World size, patch size and wrapping are edited in Settings (comes with world settings, step 12). Move or resize the view here.</p>}
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
      const val = f.name === "default" && /^-?\d+$/.test(value.trim()) ? `${value.trim()}.0` : value;
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
