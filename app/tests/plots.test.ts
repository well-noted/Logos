// Plot data (G4): a full set, then changes that add points, reset a pen or remove one.
import { test } from "node:test";
import assert from "node:assert/strict";
import { applyPlots, penColor, type PlotData, type PlotPen } from "../src/plots";

const pen = (name: string, points: [number, number][], reset = false): PlotPen => ({
  name, color: 0xffd73229, mode: 0, interval: 1, hidden: false, inLegend: true, temporary: false, reset,
  points: points.map(([x, y]) => [x, y, true, 0xffd73229] as [number, number, boolean, number]),
});
const plot = (pens: PlotPen[], extra: Partial<PlotData> = {}): PlotData =>
  ({ name: "Noise", xMin: 0, xMax: 10, yMin: 0, yMax: 100, pens, ...extra });

test("changes add points to what is there", () => {
  let p = applyPlots(new Map(), true, [plot([pen("a", [[0, 1]])])]);
  p = applyPlots(p, false, [plot([pen("a", [[1, 2], [2, 3]])], { removedPens: [] })]);
  assert.deepEqual(p.get("Noise")!.pens[0].points.map((q) => q[1]), [1, 2, 3]);
});

test("a reset pen starts again, and removed pens go", () => {
  let p = applyPlots(new Map(), true, [plot([pen("a", [[0, 1], [1, 2]]), pen("temp", [[0, 5]])])]);
  p = applyPlots(p, false, [plot([pen("a", [], true)], { yMax: 50, removedPens: ["temp"] })]);
  const n = p.get("Noise")!;
  assert.equal(n.pens.length, 1);
  assert.deepEqual(n.pens[0].points, []);
  assert.equal(n.yMax, 50);
});

test("the earlier map is left as it was", () => {
  const first = applyPlots(new Map(), true, [plot([pen("a", [[0, 1]])])]);
  applyPlots(first, false, [plot([pen("a", [[1, 2]])], { removedPens: [] })]);
  assert.equal(first.get("Noise")!.pens[0].points.length, 1);
});

test("pen colors drop the alpha byte", () => {
  assert.equal(penColor(0xffd73229), "#d73229");
  assert.equal(penColor(0xff000000), "#000000");
});
