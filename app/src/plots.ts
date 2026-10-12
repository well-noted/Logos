// Plot data from the bridge (G4): every plot once with `full`, then the changes as runs and
// commands add points. Kept outside the widget list so a run's points never touch the model
// document.

import { signal } from "@preact/signals";

export interface PlotPen {
  name: string; color: number; mode: number; interval: number; hidden: boolean;
  inLegend: boolean; temporary: boolean; reset: boolean;
  /** x, y, pen down, ARGB color. */
  points: [number, number, boolean, number][];
}

export interface PlotData {
  name: string; xMin: number; xMax: number; yMin: number; yMax: number;
  autoPlotX?: boolean; autoPlotY?: boolean; legend?: boolean;
  pens: PlotPen[];
  removedPens?: string[];
}

/** Plots by name, for the open model. Replaced (not mutated) on each change. */
export const plotData = signal<Map<string, PlotData>>(new Map());

/** Applies a `model-plots` event: everything (`full`) or the changes since the last one. */
export function applyPlots(current: Map<string, PlotData>, full: boolean, plots: PlotData[]): Map<string, PlotData> {
  if (full) return new Map(plots.map((p) => [p.name, { ...p, pens: p.pens.map((pen) => ({ ...pen })) }]));
  const next = new Map(current);
  for (const change of plots) {
    const before = next.get(change.name);
    const pens = new Map((before?.pens ?? []).map((p) => [p.name, p]));
    for (const name of change.removedPens ?? []) pens.delete(name);
    for (const pen of change.pens) {
      const old = pens.get(pen.name);
      const points = pen.reset || !old ? pen.points : old.points.concat(pen.points);
      pens.set(pen.name, { ...pen, points });
    }
    next.set(change.name, {
      ...(before ?? { name: change.name, pens: [] }),
      xMin: change.xMin, xMax: change.xMax, yMin: change.yMin, yMax: change.yMax,
      pens: [...pens.values()],
    });
  }
  return next;
}

export function clearPlots() {
  plotData.value = new Map();
}

/** "#rrggbb" for an ARGB color. */
export function penColor(argb: number): string {
  return `#${((argb >>> 0) & 0xffffff).toString(16).padStart(6, "0")}`;
}

/** Draws a plot's pens into the plotting area (x0, y0)-(x1, y1) of a canvas: lines, bars or
 * points, as NetLogo's pen modes 0, 1 and 2. */
export function drawPens(ctx: CanvasRenderingContext2D, p: PlotData, x0: number, y0: number, x1: number, y1: number) {
  const sx = (x: number) => x0 + ((x - p.xMin) / (p.xMax - p.xMin || 1)) * (x1 - x0);
  const sy = (y: number) => y1 - ((y - p.yMin) / (p.yMax - p.yMin || 1)) * (y1 - y0);
  ctx.save();
  ctx.beginPath();
  ctx.rect(x0, y0, x1 - x0, y1 - y0);
  ctx.clip();
  for (const pen of p.pens) {
    if (pen.hidden || pen.points.length === 0) continue;
    if (pen.mode === 1) {
      const w = Math.max(1, Math.abs(sx(p.xMin + pen.interval) - sx(p.xMin)));
      for (const [x, y, down, c] of pen.points) {
        if (!down) continue;
        ctx.fillStyle = penColor(c);
        const top = Math.min(sy(y), sy(0)), h = Math.abs(sy(y) - sy(0));
        ctx.fillRect(sx(x), top, w, Math.max(1, h));
      }
    } else if (pen.mode === 2) {
      for (const [x, y, down, c] of pen.points) {
        if (!down) continue;
        ctx.fillStyle = penColor(c);
        ctx.fillRect(sx(x) - 1, sy(y) - 1, 2, 2);
      }
    } else {
      ctx.lineWidth = 1;
      let prev: [number, number] | null = null;
      for (const [x, y, down, c] of pen.points) {
        const pt: [number, number] = [sx(x), sy(y)];
        // A segment is drawn when the pen is down at its end, in that point's color.
        if (prev && down) {
          ctx.strokeStyle = penColor(c);
          ctx.beginPath();
          ctx.moveTo(prev[0], prev[1]);
          ctx.lineTo(pt[0], pt[1]);
          ctx.stroke();
        }
        prev = pt;
      }
    }
  }
  ctx.restore();
}
