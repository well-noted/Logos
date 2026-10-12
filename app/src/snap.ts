// Snapping a widget to the edges of the others while it is dragged with Shift held
// (interface edit mode). Pure, so it can be tested without the app.

export interface Rect { x: number; y: number; width: number; height: number }
export interface Snapped { x: number; y: number; width: number; height: number; guidesX: number[]; guidesY: number[] }

/** How close, in pixels, an edge has to come to another widget's edge to jump to it. */
export const SNAP = 8;

function best(edges: number[], targets: number[], threshold: number): { delta: number; at: number } | null {
  let found: { delta: number; at: number } | null = null;
  for (const e of edges) {
    for (const t of targets) {
      const d = t - e;
      if (Math.abs(d) <= threshold && (!found || Math.abs(d) < Math.abs(found.delta))) found = { delta: d, at: t };
    }
  }
  return found;
}

/** Moves (or, with `resize`, resizes) `r` so its edges line up with the nearest edges of
 * `others` within `threshold`, and reports the lines it lined up on. */
export function snap(r: Rect, others: Rect[], resize = false, threshold = SNAP): Snapped {
  const xs = others.flatMap((o) => [o.x, o.x + o.width]);
  const ys = others.flatMap((o) => [o.y, o.y + o.height]);
  const out: Snapped = { ...r, guidesX: [], guidesY: [] };
  if (resize) {
    const bx = best([r.x + r.width], xs, threshold);
    if (bx) { out.width = Math.max(10, r.width + bx.delta); out.guidesX.push(bx.at); }
    const by = best([r.y + r.height], ys, threshold);
    if (by) { out.height = Math.max(10, r.height + by.delta); out.guidesY.push(by.at); }
    return out;
  }
  const bx = best([r.x, r.x + r.width], xs, threshold);
  if (bx) { out.x = Math.max(0, r.x + bx.delta); out.guidesX.push(bx.at); }
  const by = best([r.y, r.y + r.height], ys, threshold);
  if (by) { out.y = Math.max(0, r.y + by.delta); out.guidesY.push(by.at); }
  return out;
}

/** The widget under a point, last drawn first. */
export function hit<T extends Rect>(rects: T[], x: number, y: number): T | undefined {
  for (let i = rects.length - 1; i >= 0; i--) {
    const r = rects[i];
    if (x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height) return r;
  }
  return undefined;
}
