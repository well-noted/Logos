// Agent state as the host sends it (session.rs, encode_agents), decoded by frames.ts.
import { test } from "node:test";
import assert from "node:assert/strict";
import { decode } from "../src/frames";

/** Builds the host's layout: a 2 x 1 world, two turtles and `links` links. */
function agentsMessage(links: number): ArrayBuffer {
  const bytes: number[] = [];
  const pad8 = () => { while (bytes.length % 8) bytes.push(0); };
  const u32 = (v: number) => { const b = new DataView(new ArrayBuffer(4)); b.setUint32(0, v, true); bytes.push(...new Uint8Array(b.buffer)); };
  const i32 = (v: number) => { const b = new DataView(new ArrayBuffer(4)); b.setInt32(0, v, true); bytes.push(...new Uint8Array(b.buffer)); };
  const u16 = (v: number) => { const b = new DataView(new ArrayBuffer(2)); b.setUint16(0, v, true); bytes.push(...new Uint8Array(b.buffer)); };
  const f64 = (v: number) => { const b = new DataView(new ArrayBuffer(8)); b.setFloat64(0, v, true); bytes.push(...new Uint8Array(b.buffer)); };
  bytes.push(2, 0, 0, 0, 0, 0, 0, 0);
  for (const v of [3, 0]) { u32(v); }  // sequence 3 as a little-endian u64
  f64(7); f64(0);
  for (const v of [0, 1, 0, 0]) i32(v);
  u32(2); u32(0);
  bytes.push(0, 0, 0, 255, 1, 2, 3, 255);
  pad8();
  for (const col of [[0, 1], [0.5, -0.25], [0, 0], [90, 180], [1, 2.5]]) for (const v of col) f64(v);
  bytes.push(215, 50, 41, 255, 10, 20, 30, 255);
  pad8();
  for (const v of [0, 3, 2, 1]) u16(v);
  bytes.push(0, 1);
  pad8();
  u32(links); u32(0);
  for (let i = 0; i < links; i++) f64(0);
  for (let i = 0; i < links; i++) f64(1);
  for (let i = 0; i < links; i++) f64(0.25);
  for (let i = 0; i < links; i++) bytes.push(44, 209, 59, 255);
  pad8();
  for (let i = 0; i < links; i++) u16(0);
  for (let i = 0; i < links; i++) u16(1);
  for (let i = 0; i < links; i++) bytes.push(0);
  for (let i = 0; i < links; i++) bytes.push(1);
  return new Uint8Array(bytes).buffer;
}

test("turtles and links decode from the host's layout", () => {
  const a = decode(agentsMessage(1)).agents!;
  assert.equal(a.seq, 3);
  assert.equal(a.ticks, 7);
  assert.equal(a.n, 2);
  assert.deepEqual([...a.xcor], [0.5, -0.25]);
  assert.deepEqual([...a.breed], [2, 1]);
  assert.deepEqual([...a.hidden], [0, 1]);
  assert.equal(a.links.n, 1);
  assert.deepEqual([a.links.end1[0], a.links.end2[0], a.links.thickness[0]], [0, 1, 0.25]);
  assert.deepEqual([a.links.breed[0], a.links.hidden[0], a.links.directed[0]], [1, 0, 1]);
});

test("a world without links", () => {
  const a = decode(agentsMessage(0)).agents!;
  assert.equal(a.links.n, 0);
  assert.equal(a.n, 2);
});
