// The Interface tab (design 2.4, 3.2): NetLogo's toolbar, the run strip, and the model's
// widgets at their saved positions, drawn by Logos.

import { useEffect, useRef, useState } from "preact/hooks";
import { render as renderMarkdown } from "./info";
import type { Widget } from "./ipc";
import { bindCanvas, frames } from "./frames";
import { Icon } from "./icons";
import {
  inspect, model, monitors, output, pressButton, running, setSpeed, setWidgetValue, speed, stepModel,
  stopModel, ticks, viewOnTicks, viewPopped,
} from "./state";
import { ipc } from "./ipc";

/** A NetLogo color (Java ARGB integer) as CSS. */
export function argb(c: number): string {
  const v = c >>> 0;
  return `rgb(${(v >> 16) & 255},${(v >> 8) & 255},${v & 255})`;
}

function label(w: Widget, fallback: string): string {
  return w.display && w.display.length ? w.display : fallback;
}

function ticksText(t: number | null): string {
  if (t === null || t === undefined) return "";
  return Number.isInteger(t) ? String(t) : t.toFixed(2);
}

export function InterfaceTab() {
  const m = model.value;
  if (!m) return null;
  const right = Math.max(600, ...m.widgets.map((w) => w.x + w.width + 40));
  const bottom = Math.max(400, ...m.widgets.map((w) => w.y + w.height + 40));
  const oldSize = m.widgets.some((w) => w.oldSize);
  return (
    <div class="split">
      <Toolbar />
      {oldSize && (
        <div class="toolbar" role="note" style="color: var(--warning)">
          This model was converted from an older NetLogo and keeps its old widget sizes (D-18). The re-layout command comes with interface editing.
        </div>
      )}
      <div class="canvas-scroll">
        <div class="interface" style={{ width: `${right}px`, height: `${bottom}px` }} role="region" aria-label="Model interface">
          {m.widgets.map((w) => <WidgetView key={`${m.id}-${w.index}`} w={w} />)}
        </div>
      </div>
    </div>
  );
}

function Toolbar() {
  const t = ticks.value;
  const view = model.value?.widgets.find((w) => w.data.kind === "view");
  const tickLabel = view?.data.kind === "view" ? view.data.tickCounterLabel || "ticks" : "ticks";
  const isRunning = !!running.value;
  return (
    <div class="toolbar" role="toolbar" aria-label="Interface toolbar">
      <div class="group">
        <button class="btn small" disabled title="Interface editing comes in Phase 2 step 12">
          <Icon name="edit" /> Edit
        </button>
        <button class="btn small" disabled title="Interface editing comes in Phase 2 step 12">
          <Icon name="add" /> Add
        </button>
      </div>
      <span class="sep" />
      <div class="group">
        <label for="speed">Speed</label>
        <input id="speed" class="speed" type="range" min={0} max={100} step={1} value={speed.value}
          aria-valuetext={speed.value >= 100 ? "full speed" : `${speed.value} percent`}
          onInput={(e) => setSpeed(Number((e.target as HTMLInputElement).value))} />
        <span class="muted" style="width: 70px">{speed.value >= 100 ? "full speed" : speed.value <= 0 ? "slowest" : "slower"}</span>
      </div>
      <span class="sep" />
      <div class="group">
        <label for="vu">View updates</label>
        <select id="vu" value={viewOnTicks.value ? "ticks" : "iteration"}
          title="NetLogo's continuous updates redraw inside a procedure; Logos draws between iterations (D-30)"
          onChange={(e) => {
            const v = (e.target as HTMLSelectElement).value === "ticks";
            viewOnTicks.value = v;
            const m = model.value;
            if (m) ipc.viewUpdates(m.id, v);
          }}>
          <option value="ticks">on ticks</option>
          <option value="iteration">every iteration</option>
        </select>
      </div>
      <button class="icon-btn" title="Pop out view" aria-label="Pop out view" aria-pressed={viewPopped.value}
        onClick={() => { viewPopped.value = !viewPopped.value; }}>
        <Icon name="popout" />
      </button>
      <div class="runstrip" role="group" aria-label="Run">
        <span class="ticks" aria-live="off">{tickLabel}: {ticksText(t) || "–"}</span>
        <button class="icon-btn" title="Step one tick (F10)" aria-label="Step one tick" disabled={isRunning} onClick={stepModel}>
          <Icon name="step" />
        </button>
        <button class="icon-btn" title="Pause (F6)" aria-label="Pause" disabled={!isRunning} onClick={stopModel}>
          <Icon name="pause" />
        </button>
        <span role="status" class="muted" style="width: 64px">{isRunning ? "running" : "stopped"}</span>
      </div>
    </div>
  );
}

function WidgetView({ w }: { w: Widget }) {
  const style = { left: `${w.x}px`, top: `${w.y}px`, width: `${w.width}px`, height: `${w.height}px` };
  const cls = `w${w.oldSize ? " old-size" : ""}`;
  const d = w.data;
  switch (d.kind) {
    case "view": return <ViewWidget w={w} style={style} cls={cls} />;
    case "button": {
      const isRunning = running.value?.button === w.index;
      const blocked = d.disableUntilTicks && (ticks.value === null || ticks.value === undefined);
      const name = label(w, d.source.trim().split("\n")[0] || "button");
      return (
        <button class={`${cls} w-button`} style={style} aria-pressed={d.forever ? isRunning : undefined}
          disabled={blocked} title={blocked ? "Available after the ticks start (reset-ticks)" : d.source}
          aria-label={`${name}${d.forever ? ", forever button" : ""}`}
          onClick={() => pressButton(w.index)}
          accessKey={d.actionKey ?? undefined}>
          <span>{name}</span>
          {d.forever && <Icon name="forever" />}
          {d.actionKey && <span class="key" aria-hidden="true">{d.actionKey}</span>}
        </button>
      );
    }
    case "slider": return <SliderWidget w={w} style={style} cls={cls} />;
    case "switch":
      return (
        <div class={`${cls} w-switch`} style={style} role="switch" tabIndex={0} aria-checked={d.on}
          aria-label={label(w, d.variable)}
          onClick={() => setWidgetValue(w.index, { kind: "bool", value: !d.on })}
          onKeyDown={(e) => { if (e.key === " " || e.key === "Enter") { e.preventDefault(); setWidgetValue(w.index, { kind: "bool", value: !d.on }); } }}>
          <span class="track" aria-hidden="true" />
          <span>{label(w, d.variable)}</span>
        </div>
      );
    case "chooser": {
      const id = `w-${w.index}`;
      return (
        <div class={`${cls} w-chooser`} style={style}>
          <label class="label" for={id}>{label(w, d.variable)}</label>
          <select id={id} value={String(d.current)}
            onChange={(e) => setWidgetValue(w.index, { kind: "choice", value: Number((e.target as HTMLSelectElement).value) })}>
            {d.choices.map((c, i) => <option value={String(i)}>{c.label}</option>)}
          </select>
        </div>
      );
    }
    case "input": return <InputWidget w={w} style={style} cls={cls} />;
    case "monitor": {
      const v = monitors.value[w.index];
      const name = label(w, d.source.trim());
      return (
        <div class={`${cls} w-monitor`} style={style} role="group" aria-label={`Monitor ${name}`}>
          <span class="label">{name}</span>
          <span class={`value${v?.error ? " error" : ""}`} aria-live="off" title={v?.text}>
            {v?.text ?? ""}{d.units && v ? ` ${d.units}` : ""}
          </span>
        </div>
      );
    }
    case "plot": return <PlotWidget w={w} style={style} cls={cls} />;
    case "output":
      return (
        <div class={`${cls} w-output`} style={style} role="log" aria-label="Model output">
          <pre style={{ fontSize: `${d.fontSize}px` }}>{output.value}</pre>
        </div>
      );
    case "note":
      return d.markdown
        ? <div class={`${cls} w-note`} style={{ ...style, fontSize: `${d.fontSize}px` }}
            dangerouslySetInnerHTML={{ __html: renderMarkdown(d.text) }} />
        : <div class={`${cls} w-note`} style={{ ...style, fontSize: `${d.fontSize}px`, color: d.textColorLight != null ? argb(d.textColorLight) : undefined }}>{d.text}</div>;
    default:
      return <div class={`${cls} w-unknown`} style={style} title={`<${w.element}> isn't a widget NetLogo 7.0.4 reads; Logos keeps it unchanged`}>{w.element}</div>;
  }
}

type WProps = { w: Widget; style: Record<string, string>; cls: string };

function SliderWidget({ w, style, cls }: WProps) {
  const d = w.data;
  const [local, setLocal] = useState<number | null>(null);
  if (d.kind !== "slider") return null;
  const min = Number(d.min), max = Number(d.max), step = Number(d.step);
  const numeric = Number.isFinite(min) && Number.isFinite(max) && Number.isFinite(step) && step > 0;
  const value = local ?? d.value;
  const decimals = numeric ? Math.max(0, (String(d.step).split(".")[1] ?? "").length) : 3;
  const shown = Number(value).toFixed(decimals).replace(/\.?0+$/, (m) => (decimals > 0 && m.length ? "" : m));
  const name = label(w, d.variable);
  const id = `w-${w.index}`;
  const inner = d.vertical
    ? { position: "absolute", left: "0", top: `${w.height}px`, width: `${w.height}px`, height: `${w.width}px`,
        transform: "rotate(-90deg)", transformOrigin: "0 0", padding: "4px 7px", display: "flex",
        flexDirection: "column", justifyContent: "center", gap: "2px" }
    : undefined;
  const body = (
    <>
      <div class="row">
        <label class="label" for={id}>{name}</label>
        <span class="val">{numeric ? Number(value).toFixed(decimals) : value}{d.units ? ` ${d.units}` : ""}</span>
      </div>
      {numeric
        ? <input id={id} type="range" min={min} max={max} step={step} value={value}
            aria-valuetext={`${shown}${d.units ? " " + d.units : ""}`}
            onInput={(e) => setLocal(Number((e.target as HTMLInputElement).value))}
            onChange={(e) => { const v = Number((e.target as HTMLInputElement).value); setLocal(null); setWidgetValue(w.index, { kind: "number", value: v }); }} />
        : <span class="muted" title={`min ${d.min}, max ${d.max}, step ${d.step}`}>range set by reporters</span>}
    </>
  );
  return (
    <div class={`${cls} w-slider`} style={d.vertical ? { ...style, padding: "0" } : style}>
      {inner ? <div style={inner}>{body}</div> : body}
    </div>
  );
}

function InputWidget({ w, style, cls }: WProps) {
  const d = w.data;
  const initial = d.kind === "input" ? d.value : "";
  const [text, setText] = useState(initial);
  useEffect(() => setText(initial), [initial]);
  if (d.kind !== "input") return null;
  const commit = () => { if (text !== d.value) setWidgetValue(w.index, { kind: "text", value: text }); };
  const id = `w-${w.index}`;
  return (
    <div class={`${cls} w-input`} style={style}>
      <label class="label" for={id}>{label(w, d.variable)} <span class="muted">({d.inputType})</span></label>
      {d.multiline
        ? <textarea id={id} value={text} onInput={(e) => setText((e.target as HTMLTextAreaElement).value)} onBlur={commit} />
        : <input id={id} type="text" value={text} onInput={(e) => setText((e.target as HTMLInputElement).value)}
            onBlur={commit} onKeyDown={(e) => { if (e.key === "Enter") commit(); }} />}
    </div>
  );
}

function PlotWidget({ w, style, cls }: WProps) {
  const d = w.data;
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const c = ref.current;
    if (!c || d.kind !== "plot") return;
    const r = c.getBoundingClientRect();
    c.width = Math.max(10, Math.floor(r.width));
    c.height = Math.max(10, Math.floor(r.height));
    const ctx = c.getContext("2d")!;
    const css = getComputedStyle(document.documentElement);
    const fg = css.getPropertyValue("--widget-text").trim() || "#000";
    const muted = css.getPropertyValue("--text-muted").trim() || "#666";
    ctx.clearRect(0, 0, c.width, c.height);
    const l = 30, b = 16, t = 4, rr = 6;
    ctx.strokeStyle = fg;
    ctx.strokeRect(l + 0.5, t + 0.5, c.width - l - rr, c.height - t - b);
    ctx.fillStyle = muted;
    ctx.font = "10px sans-serif";
    ctx.fillText(String(d.yMax), 2, t + 9);
    ctx.fillText(String(d.yMin), 2, c.height - b);
    ctx.fillText(String(d.xMin), l, c.height - 4);
    const xm = String(d.xMax);
    ctx.fillText(xm, c.width - rr - ctx.measureText(xm).width, c.height - 4);
    ctx.textAlign = "center";
    ctx.fillText("No plot data yet", (c.width + l) / 2, (c.height - b + t) / 2 - 6);
    ctx.fillText("(needs the plot stream, G4)", (c.width + l) / 2, (c.height - b + t) / 2 + 8);
  }, [d, w.width, w.height]);
  if (d.kind !== "plot") return null;
  const name = label(w, "plot");
  return (
    <div class={`${cls} w-plot`} style={style} role="img" aria-label={`Plot ${name}: ${d.pens.map((p) => p.display).join(", ")}. Plot data is not shown yet.`}>
      <div class="title">{name}</div>
      <canvas ref={ref} aria-hidden="true" />
      {d.legend && d.pens.length > 0 && (
        <div class="legend" aria-hidden="true">
          {d.pens.filter((p) => p.legend).map((p) => <span><i style={{ background: argb(p.color) }} />{p.display}</span>)}
        </div>
      )}
    </div>
  );
}

function ViewWidget({ w, style, cls }: WProps) {
  const d = w.data;
  const ref = useRef<HTMLCanvasElement>(null);
  const [hasFrame, setHasFrame] = useState(!!frames.view);
  const [tip, setTip] = useState<{ x: number; y: number; text: string } | null>(null);
  useEffect(() => {
    const c = ref.current;
    if (!c) return;
    const unbind = bindCanvas(c);
    const off = frames.subscribe(() => setHasFrame(true));
    return () => { unbind(); off(); };
  }, []);
  if (d.kind !== "view") return null;
  const toPatch = (e: MouseEvent) => {
    const c = ref.current!;
    const r = c.getBoundingClientRect();
    const fx = (e.clientX - r.left) / r.width, fy = (e.clientY - r.top) / r.height;
    const x = d.minPxcor - 0.5 + fx * (d.maxPxcor - d.minPxcor + 1);
    const y = d.maxPycor + 0.5 - fy * (d.maxPycor - d.minPycor + 1);
    return { x, y };
  };
  const onMove = (e: MouseEvent) => {
    const { x, y } = toPatch(e);
    const i = frames.turtleAt(x, y, 1);
    const a = frames.agents;
    const px = Math.round(x), py = Math.round(y);
    const text = i !== null && a
      ? `turtle ${a.who[i]}  breed ${a.breed[i]}  (${a.xcor[i].toFixed(2)}, ${a.ycor[i].toFixed(2)})`
      : `patch ${px} ${py}`;
    setTip({ x: e.clientX + 12, y: e.clientY + 12, text });
  };
  const onClick = (e: MouseEvent) => {
    const { x, y } = toPatch(e);
    const i = frames.turtleAt(x, y, 1);
    const a = frames.agents;
    if (i !== null && a) inspect({ kind: "turtle", who: a.who[i] });
    else inspect({ kind: "patch", x: Math.round(x), y: Math.round(y) });
  };
  const counts = [...frames.countsByBreed().entries()].map(([b, n]) => `${n} ${b === 0 ? "turtles" : `of breed ${b}`}`).join(", ");
  return (
    <div class={`${cls} w-view`} style={style}>
      {d.showTickCounter && (
        <div class="tickbar"><span>{d.tickCounterLabel || "ticks"}: {ticksText(ticks.value)}</span></div>
      )}
      <canvas ref={ref} role="img" tabIndex={0}
        aria-label={`Live view, ${d.maxPxcor - d.minPxcor + 1} by ${d.maxPycor - d.minPycor + 1} patches. ${counts || "No agent data yet."} Click an agent to inspect it.`}
        onMouseMove={onMove} onMouseLeave={() => setTip(null)} onClick={onClick} />
      {!hasFrame && <div class="placeholder">The view appears when NetLogo has drawn it.</div>}
      {tip && <div class="view-tip" style={{ left: `${tip.x}px`, top: `${tip.y}px` }}>{tip.text}</div>}
    </div>
  );
}

/** The same frames in a movable panel (D-22). */
export function PopOutView() {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => { if (ref.current) return bindCanvas(ref.current); }, []);
  return (
    <div class="popout" role="dialog" aria-label="View">
      <header>
        <span>View{model.value ? ` - ${model.value.name}` : ""}</span>
        <button class="icon-btn" aria-label="Close the pop-out view" onClick={() => { viewPopped.value = false; }}><Icon name="close" /></button>
      </header>
      <canvas ref={ref} role="img" aria-label="Live view" />
    </div>
  );
}
