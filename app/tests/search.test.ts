// Part R: search and replace across a model.
import { test } from "node:test";
import assert from "node:assert/strict";
import { compile, findAll, replaceRanges, searchModel } from "../src/search";
import type { Widget } from "../src/ipc";

const opts = { caseSensitive: false, wholeWord: false, regex: false };

function re(q: string, o = opts): RegExp {
  const r = compile(q, o);
  assert.ok(r instanceof RegExp, String(r));
  return r as RegExp;
}

test("plain search escapes regular expression characters", () => {
  assert.deepEqual(findAll("a.b axb", re("a.b")), [[0, 3]]);
  assert.deepEqual(findAll("Go go GO", re("go", { ...opts, caseSensitive: true })), [[3, 5]]);
});

test("whole words follow NetLogo's names", () => {
  const text = "go go-once go? to-go [go] go";
  assert.deepEqual(findAll(text, re("go", { ...opts, wholeWord: true })), [[0, 2], [22, 24], [26, 28]]);
});

test("invalid regular expressions report an error", () => {
  assert.equal(typeof compile("a(", { ...opts, regex: true }), "string");
  assert.equal(compile("", opts), "");
});

test("replace keeps everything else and fills in groups", () => {
  const text = "set fertility 0.1\nset fertility-ends-age 30";
  const r = re("fertility", { ...opts, wholeWord: true });
  assert.equal(replaceRanges(text, findAll(text, r), r, "birth-rate", opts), "set birth-rate 0.1\nset fertility-ends-age 30");
  const rx = re("set (\\S+)", { ...opts, regex: true });
  assert.equal(replaceRanges(text, findAll(text, rx), rx, "let $1", { ...opts, regex: true }), "let fertility 0.1\nlet fertility-ends-age 30");
});

test("search covers code, info and widget fields, with lines", () => {
  const widgets: Widget[] = [
    { index: 0, element: "button", x: 0, y: 0, width: 10, height: 10, oldSize: false, display: null, problems: [],
      data: { kind: "button", source: "go", forever: true, agentKind: "Observer", actionKey: null, disableUntilTicks: false } },
    { index: 1, element: "plot", x: 0, y: 0, width: 10, height: 10, oldSize: false, display: "Population", problems: [],
      data: { kind: "plot", xAxis: null, yAxis: null, xMin: 0, xMax: 1, yMin: 0, yMax: 1, autoPlotX: true, autoPlotY: true,
        legend: false, setup: "", update: "", pens: [{ display: "p", interval: 1, mode: 0, color: 0, legend: true, setup: "", update: "plot count go" }] } },
  ];
  const ms = searchModel({ code: "to setup\nend\nto go\nend", info: "Press go now", widgets }, re("go", { ...opts, wholeWord: true }));
  assert.deepEqual(ms.map((m) => [m.place.kind, m.line, m.col]), [["code", 3, 3], ["info", 1, 6], ["widget", 1, 0], ["widget", 1, 11]]);
  const last = ms[3].place;
  assert.ok(last.kind === "widget" && last.field === "part:pen.0/update");
});
