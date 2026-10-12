// Interface edit mode: Shift-drag snapping and finding the widget under the pointer.
import { test } from "node:test";
import assert from "node:assert/strict";
import { hit, snap } from "../src/snap";

const others = [{ x: 10, y: 10, width: 100, height: 50 }, { x: 200, y: 100, width: 80, height: 40 }];

test("a moved widget jumps to the nearest edge within reach", () => {
  const s = snap({ x: 14, y: 70, width: 60, height: 30 }, others);
  assert.equal(s.x, 10);
  assert.deepEqual(s.guidesX, [10]);
  // Its bottom (100) lines up with the second widget's top.
  assert.equal(s.y, 70);
  assert.deepEqual(s.guidesY, [100]);
});

test("nothing near: no change", () => {
  const s = snap({ x: 400, y: 400, width: 20, height: 20 }, others);
  assert.deepEqual([s.x, s.y, s.guidesX.length, s.guidesY.length], [400, 400, 0, 0]);
});

test("resizing snaps the right and bottom edges", () => {
  const s = snap({ x: 0, y: 0, width: 195, height: 97 }, others, true);
  assert.deepEqual([s.width, s.height], [200, 100]);
});

test("hit finds the widget drawn last", () => {
  const rects = [{ x: 0, y: 0, width: 50, height: 50, id: "a" }, { x: 20, y: 20, width: 50, height: 50, id: "b" }];
  assert.equal(hit(rects, 30, 30)?.id, "b");
  assert.equal(hit(rects, 5, 5)?.id, "a");
  assert.equal(hit(rects, 90, 90), undefined);
});
