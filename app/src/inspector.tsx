// The right panel's inspectors (design 4.4): an agent's variables, watches with sparklines,
// and "Pause when". Everything here reads through workspace.observe (D-14); editing a value
// is a command, which runs between iterations when the model is running (G1).

import { useEffect, useState } from "preact/hooks";
import { errorText, ipc } from "./ipc";
import { PropertySheet, editMode } from "./editor";
import { Icon } from "./icons";
import {
  inspected, model, monitors, outline, pauseNote, pauseWhen, rightOpen, runCommand, ticks, watches,
} from "./state";

const TURTLE_VARS = ["who", "color", "heading", "xcor", "ycor", "shape", "label", "label-color", "breed", "hidden?", "size", "pen-size", "pen-mode"];
const PATCH_VARS = ["pxcor", "pycor", "pcolor", "plabel", "plabel-color"];

/** `-own` variables declared in the code, from the outline: turtles-own, patches-own, breeds. */
function declared(kind: "turtle" | "patch"): string[] {
  const out: string[] = [];
  for (const e of outline.value) {
    if (e.kind !== "variables" || !e.detail) continue;
    const kw = e.name.toLowerCase();
    if (kind === "patch" && kw === "patches-own") out.push(...e.detail.split(/\s+/));
    if (kind === "turtle" && kw.endsWith("-own") && kw !== "patches-own" && kw !== "links-own") {
      // Breed variables only exist for that breed; the bridge reports an error for others,
      // and the row then shows nothing.
      out.push(...e.detail.split(/\s+/));
    }
  }
  return [...new Set(out.filter(Boolean))];
}

export function RightPanel() {
  return (
    <aside class="rightpanel" aria-label="Inspectors">
      <div class="panel-head">
        <span>{editMode.value ? "Properties" : "Inspectors"}</span>
        <button class="icon-btn" aria-label="Close inspectors" onClick={() => { rightOpen.value = false; }}><Icon name="close" /></button>
      </div>
      <div class="panel-body">
        {editMode.value ? <PropertySheet /> : (
          <>
            <AgentInspector />
            <Watches />
            <PauseWhen />
          </>
        )}
      </div>
    </aside>
  );
}

function AgentInspector() {
  const target = inspected.value;
  const m = model.value;
  const [rows, setRows] = useState<{ name: string; text: string; ok: boolean }[]>([]);
  const [editing, setEditing] = useState<{ name: string; text: string } | null>(null);
  const who = target?.kind === "turtle" ? `turtle ${target.who}` : target ? `patch ${target.x} ${target.y}` : "";
  const names = target?.kind === "turtle" ? [...TURTLE_VARS, ...declared("turtle")] : target ? [...PATCH_VARS, ...declared("patch")] : [];
  // Refresh when monitors refresh (each update during a run, after each command).
  const pulse = monitors.value;
  const t = ticks.value;
  useEffect(() => {
    if (!m || !target) return;
    let cancelled = false;
    ipc.observe(m.id, names.map((n) => `[${n}] of ${who}`)).then((r) => {
      if (!cancelled) setRows(names.map((name, i) => ({ name, text: r.values[i]?.text ?? "", ok: r.values[i]?.ok ?? false })));
    }).catch(() => {});
    return () => { cancelled = true; };
  }, [m?.id, who, pulse, t]);
  if (!target) {
    return <p class="empty-note">Click an agent in the view to inspect it.</p>;
  }
  const commit = async () => {
    if (!editing) return;
    await runCommand(`ask ${who} [ set ${editing.name} ${editing.text} ]`, "observer");
    setEditing(null);
  };
  return (
    <section class="inspector" aria-label={`Inspector for ${who}`}>
      <div style="display:flex;justify-content:space-between;align-items:center">
        <strong>{who}</strong>
        <button class="icon-btn" aria-label="Stop inspecting" onClick={() => { inspected.value = null; }}><Icon name="close" /></button>
      </div>
      <table>
        <tbody>
          {rows.filter((r) => r.ok).map((r) => (
            <tr>
              <td>{r.name}</td>
              <td>
                {editing?.name === r.name
                  ? <input type="text" value={editing.text} aria-label={`New value for ${r.name}`} autoFocus
                      onInput={(e) => setEditing({ name: r.name, text: (e.target as HTMLInputElement).value })}
                      onKeyDown={(e) => { if (e.key === "Enter") commit(); if (e.key === "Escape") setEditing(null); }} />
                  : <button class="btn small" style="border:0;background:transparent;padding:0;height:auto"
                      title="Edit (runs a command, between iterations when running)"
                      onClick={() => setEditing({ name: r.name, text: r.text })}>{r.text}</button>}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <button class="btn small" onClick={() => {
        const r = target.kind === "turtle" ? `[ycor] of turtle ${target.who}` : `[pcolor] of patch ${target.x} ${target.y}`;
        addWatch(r);
      }}>Watch a variable…</button>
    </section>
  );
}

function addWatch(reporter: string) {
  if (!reporter.trim() || watches.value.some((w) => w.reporter === reporter)) return;
  watches.value = [...watches.value, { reporter, values: [], text: "", ok: true }];
}

function Spark({ values }: { values: number[] }) {
  if (values.length < 2) return <svg aria-hidden="true" />;
  const min = Math.min(...values), max = Math.max(...values);
  const span = max - min || 1;
  const pts = values.map((v, i) => `${(i / (values.length - 1)) * 100},${24 - ((v - min) / span) * 22}`).join(" ");
  return (
    <svg viewBox="0 0 100 26" preserveAspectRatio="none" aria-hidden="true">
      <polyline points={pts} fill="none" stroke="currentColor" stroke-width="1.2" vector-effect="non-scaling-stroke" />
    </svg>
  );
}

function Watches() {
  const m = model.value;
  const [draft, setDraft] = useState("");
  const pulse = monitors.value;
  const t = ticks.value;
  useEffect(() => {
    const list = watches.value;
    if (!m || list.length === 0) return;
    ipc.observe(m.id, list.map((w) => w.reporter)).then((r) => {
      watches.value = list.map((w, i) => {
        const v = r.values[i];
        const n = Number(v?.text);
        return {
          ...w, text: v?.text ?? "", ok: v?.ok ?? false,
          values: v?.ok && Number.isFinite(n) ? [...w.values, n].slice(-200) : w.values,
        };
      });
    }).catch(() => {});
  }, [m?.id, pulse, t]);
  return (
    <section aria-label="Watches" style="margin-top: 10px">
      <div class="panel-head" style="padding: 0"><span>Watches</span></div>
      {watches.value.map((w) => (
        <div class="watch">
          <span class="r" title={w.reporter}>{w.reporter}</span>
          <span class={`mono ${w.ok ? "" : "err"}`}>{w.text}
            <button class="icon-btn" aria-label={`Remove watch ${w.reporter}`}
              onClick={() => { watches.value = watches.value.filter((x) => x !== w); }}><Icon name="close" /></button>
          </span>
          <Spark values={w.values} />
        </div>
      ))}
      <form style="display:flex;gap:4px;margin-top:6px" onSubmit={(e) => { e.preventDefault(); addWatch(draft); setDraft(""); }}>
        <input type="text" class="mono" style="flex:1" placeholder="reporter, e.g. count turtles" aria-label="Reporter to watch"
          value={draft} onInput={(e) => setDraft((e.target as HTMLInputElement).value)} />
        <button class="btn small" type="submit">Add</button>
      </form>
    </section>
  );
}

function PauseWhen() {
  const m = model.value;
  const [error, setError] = useState("");
  // The host makes it the run's stop condition (G6), checked after every iteration; a run in
  // progress takes a change from its next iteration.
  useEffect(() => {
    if (!m) return;
    const cond = pauseWhen.value;
    const timer = setTimeout(() => {
      ipc.pauseWhen(m.id, cond).then(() => setError("")).catch((e) => setError(errorText(e)));
    }, 400);
    return () => clearTimeout(timer);
  }, [pauseWhen.value, m?.id]);
  return (
    <section aria-label="Pause when" style="margin-top: 10px">
      <div class="panel-head" style="padding: 0"><span>Pause when</span></div>
      <input type="text" class="mono" style="width:100%" placeholder="e.g. count turtles < 5" aria-label="Pause when this reports true"
        value={pauseWhen.value} onInput={(e) => { pauseWhen.value = (e.target as HTMLInputElement).value; pauseNote.value = ""; }} />
      <p class="empty-note" style="padding: 4px 0">
        Checked after every iteration of a forever button. The run stops after the iteration where it reports true.
      </p>
      {error && <p class="empty-note" role="alert" style="padding: 0; color: var(--error)">{error}</p>}
      {pauseNote.value && <p class="empty-note" role="status" style="padding: 0; color: var(--warning)">{pauseNote.value}</p>}
    </section>
  );
}
