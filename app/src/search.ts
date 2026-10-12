// Search and replace across a model (spec part R): its code, its Info text and the text in
// its widgets. Pure functions, so they can be tested without the app.

import type { Widget } from "./ipc";

export interface SearchOptions { caseSensitive: boolean; wholeWord: boolean; regex: boolean }

/** Where a match is. A widget field is `attr:<name>`, `text` (a button's commands, a
 * monitor's reporter, a note's text) or `part:<path>` (a plot's or a pen's commands). */
export type Place =
  | { kind: "code" }
  | { kind: "info" }
  | { kind: "widget"; index: number; field: string; label: string };

export interface Match {
  id: string;
  place: Place;
  /** Offsets in the searched text. */
  start: number;
  end: number;
  /** 1-based line within the searched text, and that line's text. */
  line: number;
  lineText: string;
  /** Offset of the match within lineText. */
  col: number;
}

export interface Searchable { code: string; info: string; widgets: Widget[] }

/** Characters NetLogo allows in names besides letters and digits; whole-word matching treats
 * them as part of a word, so `go` doesn't match inside `go-once` or `go?`. */
const WORD = "[\\p{L}\\p{N}_\\-?!.=*<>:#+/%$^'&]";

/** The regular expression for a query, or an error message. */
export function compile(query: string, opts: SearchOptions): RegExp | string {
  if (!query) return "";
  let source = opts.regex ? query : query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  if (opts.wholeWord) source = `(?<!${WORD})(?:${source})(?!${WORD})`;
  try {
    return new RegExp(source, `gu${opts.caseSensitive ? "" : "i"}`);
  } catch (e) {
    return (e as Error).message;
  }
}

/** Ranges of every match in a text. Empty matches are skipped. */
export function findAll(text: string, re: RegExp): [number, number][] {
  const out: [number, number][] = [];
  re.lastIndex = 0;
  for (const m of text.matchAll(re)) {
    if (m[0].length === 0) continue;
    out.push([m.index!, m.index! + m[0].length]);
  }
  return out;
}

/** The text of each searchable widget field. */
export function widgetFields(w: Widget): { field: string; label: string; text: string }[] {
  const d = w.data;
  const name = w.display || ("variable" in d ? d.variable : "") || w.element;
  const out: { field: string; label: string; text: string }[] = [];
  const add = (field: string, what: string, text: string | null | undefined) => {
    if (text) out.push({ field, label: `${w.element} ${name}: ${what}`, text });
  };
  add("attr:display", "label", w.display);
  switch (d.kind) {
    case "button": add("text", "commands", d.source); break;
    case "monitor": add("text", "reporter", d.source); add("attr:units", "units", d.units); break;
    case "note": add("text", "text", d.text); break;
    case "slider":
      add("attr:variable", "variable", d.variable);
      add("attr:min", "minimum", d.min); add("attr:max", "maximum", d.max); add("attr:step", "increment", d.step);
      add("attr:units", "units", d.units);
      break;
    case "switch": case "chooser": case "input": add("attr:variable", "variable", d.variable); break;
    case "plot":
      add("attr:xAxis", "x axis label", d.xAxis); add("attr:yAxis", "y axis label", d.yAxis);
      add("part:setup", "setup commands", d.setup); add("part:update", "update commands", d.update);
      d.pens.forEach((p, i) => {
        add(`part:pen.${i}/setup`, `pen ${p.display} setup`, p.setup);
        add(`part:pen.${i}/update`, `pen ${p.display} update`, p.update);
      });
      break;
    case "view": add("attr:tickCounterLabel", "tick counter label", d.tickCounterLabel); break;
    default: break;
  }
  return out;
}

function matchesIn(text: string, re: RegExp, place: Place, prefix: string): Match[] {
  const ranges = findAll(text, re);
  if (ranges.length === 0) return [];
  const lineStarts = [0];
  for (let i = 0; i < text.length; i++) if (text[i] === "\n") lineStarts.push(i + 1);
  return ranges.map(([start, end]) => {
    let lo = 0, hi = lineStarts.length - 1;
    while (lo < hi) { const mid = (lo + hi + 1) >> 1; if (lineStarts[mid] <= start) lo = mid; else hi = mid - 1; }
    const ls = lineStarts[lo];
    const le = text.indexOf("\n", ls);
    const lineText = text.slice(ls, le === -1 ? text.length : le).replace(/\r$/, "");
    return { id: `${prefix}:${start}`, place, start, end, line: lo + 1, lineText, col: start - ls };
  });
}

export function searchModel(m: Searchable, re: RegExp): Match[] {
  const out = [
    ...matchesIn(m.code, re, { kind: "code" }, "code"),
    ...matchesIn(m.info, re, { kind: "info" }, "info"),
  ];
  for (const w of m.widgets) {
    for (const f of widgetFields(w)) {
      out.push(...matchesIn(f.text, re, { kind: "widget", index: w.index, field: f.field, label: f.label }, `w${w.index}.${f.field}`));
    }
  }
  return out;
}

/** What one match becomes: the replacement, with `$1` and the like filled in for a regular
 * expression. */
export function replacementFor(matched: string, re: RegExp, replacement: string, opts: SearchOptions): string {
  if (!opts.regex) return replacement;
  const single = new RegExp(re.source, re.flags.replace("g", ""));
  return matched.replace(single, replacement);
}

/** A text with the chosen matches replaced. Ranges must come from the same text. */
export function replaceRanges(text: string, ranges: [number, number][], re: RegExp, replacement: string, opts: SearchOptions): string {
  const sorted = [...ranges].sort((a, b) => b[0] - a[0]);
  let out = text;
  for (const [s, e] of sorted) out = out.slice(0, s) + replacementFor(text.slice(s, e), re, replacement, opts) + out.slice(e);
  return out;
}

/** The current text of a widget field, for applying replacements. */
export function fieldText(w: Widget, field: string): string | undefined {
  return widgetFields(w).find((f) => f.field === field)?.text;
}
