// The live view's data: the latest view frame and agent state from the host, decoded into
// typed arrays without copying (same layout as spike F1). Canvases draw the latest frame on
// each animation frame; several canvases (the Interface tab's view and the pop-out) share one
// stream (D-22).

const HEADER = 32;

export interface ViewFrame {
  seq: number; ticks: number | null; width: number; height: number; pixels: Uint8ClampedArray<ArrayBuffer>;
}

export interface AgentState {
  seq: number; ticks: number | null;
  minPx: number; maxPx: number; minPy: number; maxPy: number; pw: number; ph: number;
  n: number; patches: Uint8ClampedArray<ArrayBuffer>;
  who: Float64Array; xcor: Float64Array; ycor: Float64Array; heading: Float64Array; size: Float64Array;
  color: Uint32Array; shape: Uint16Array; breed: Uint16Array; hidden: Uint8Array;
}

export function decode(buf: ArrayBuffer): { view?: ViewFrame; agents?: AgentState } {
  const dv = new DataView(buf);
  const kind = dv.getUint8(0);
  const seq = Number(dv.getBigUint64(8, true));
  const t = dv.getFloat64(16, true);
  const ticks = Number.isNaN(t) ? null : t;
  if (kind === 1) {
    const width = dv.getUint32(HEADER, true);
    const height = dv.getUint32(HEADER + 4, true);
    return { view: { seq, ticks, width, height, pixels: new Uint8ClampedArray(buf, HEADER + 8, width * height * 4) } };
  }
  const minPx = dv.getInt32(HEADER, true), maxPx = dv.getInt32(HEADER + 4, true);
  const minPy = dv.getInt32(HEADER + 8, true), maxPy = dv.getInt32(HEADER + 12, true);
  const n = dv.getUint32(HEADER + 16, true);
  const pw = maxPx - minPx + 1, ph = maxPy - minPy + 1;
  let off = HEADER + 24;
  const patches = new Uint8ClampedArray(buf, off, pw * ph * 4);
  off = Math.ceil((off + pw * ph * 4) / 8) * 8;
  const f64 = () => { const a = new Float64Array(buf, off, n); off += n * 8; return a; };
  const who = f64(), xcor = f64(), ycor = f64(), heading = f64(), size = f64();
  const color = new Uint32Array(buf, off, n);
  off = Math.ceil((off + n * 4) / 8) * 8;
  const shape = new Uint16Array(buf, off, n); off += n * 2;
  const breed = new Uint16Array(buf, off, n); off += n * 2;
  const hidden = new Uint8Array(buf, off, n);
  return { agents: { seq, ticks, minPx, maxPx, minPy, maxPy, pw, ph, n, patches, who, xcor, ycor, heading, size, color, shape, breed, hidden } };
}

type Listener = () => void;

class FrameStore {
  view: ViewFrame | null = null;
  agents: AgentState | null = null;
  /** Increases with every new view frame, so canvases know when to redraw. */
  version = 0;
  agentVersion = 0;
  private listeners = new Set<Listener>();

  receive = (buf: ArrayBuffer) => {
    if (!buf || buf.byteLength < HEADER) return;
    const d = decode(buf);
    if (d.view) { this.view = d.view; this.version++; }
    if (d.agents) { this.agents = d.agents; this.agentVersion++; }
    for (const l of this.listeners) l();
  };

  reset() {
    this.view = null; this.agents = null; this.version++; this.agentVersion++;
  }

  subscribe(l: Listener): () => void {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  }

  /** The turtle nearest to a point in patch coordinates, within `radius` patches. */
  turtleAt(x: number, y: number, radius = 1): number | null {
    const a = this.agents;
    if (!a) return null;
    let best = -1, bestD = radius * radius;
    for (let i = 0; i < a.n; i++) {
      if (a.hidden[i]) continue;
      const dx = a.xcor[i] - x, dy = a.ycor[i] - y;
      const d = dx * dx + dy * dy;
      if (d <= bestD) { bestD = d; best = i; }
    }
    return best < 0 ? null : best;
  }

  /** Turtle counts by breed index. */
  countsByBreed(): Map<number, number> {
    const out = new Map<number, number>();
    const a = this.agents;
    if (!a) return out;
    for (let i = 0; i < a.n; i++) out.set(a.breed[i], (out.get(a.breed[i]) ?? 0) + 1);
    return out;
  }
}

export const frames = new FrameStore();

/** Draws the latest view frame into a canvas whenever a new one arrives. Returns a cleanup. */
export function bindCanvas(canvas: HTMLCanvasElement): () => void {
  const ctx = canvas.getContext("2d", { alpha: false });
  let drawn = -1;
  let raf = 0;
  const draw = () => {
    raf = 0;
    const f = frames.view;
    if (!ctx || !f || drawn === frames.version) return;
    drawn = frames.version;
    if (canvas.width !== f.width || canvas.height !== f.height) {
      canvas.width = f.width;
      canvas.height = f.height;
    }
    ctx.putImageData(new ImageData(f.pixels, f.width, f.height), 0, 0);
  };
  const schedule = () => { if (!raf) raf = requestAnimationFrame(draw); };
  const off = frames.subscribe(schedule);
  schedule();
  return () => { off(); if (raf) cancelAnimationFrame(raf); };
}
