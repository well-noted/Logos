// The Search activity (spec part R): search and replace across the model's code, Info and
// widgets. Results update as the model changes; with a replacement typed, each result shows
// what it becomes, and the checked ones are replaced together. `.nls` files are searched once
// the app opens them.

import { signal } from "@preact/signals";
import { useMemo, useState } from "preact/hooks";
import { changeOpenCode, revealRange } from "./code";
import { type Op, editMode, editWidget, selected } from "./editor";
import { addActivity, model, modelTab, rightOpen, setCode, setInfo, showToast } from "./state";
import {
  type Match, type SearchOptions, compile, fieldText, replaceRanges, replacementFor, searchModel,
} from "./search";

const query = signal("");
const replacement = signal("");
const options = signal<SearchOptions>({ caseSensitive: false, wholeWord: false, regex: false });

function placeName(m: Match): string {
  return m.place.kind === "code" ? "Code" : m.place.kind === "info" ? "Info" : m.place.label;
}

function reveal(m: Match) {
  if (m.place.kind === "code") {
    modelTab.value = "code";
    setTimeout(() => revealRange(m.start, m.end), 60);
  } else if (m.place.kind === "info") {
    modelTab.value = "info";
  } else {
    modelTab.value = "interface";
    editMode.value = true;
    selected.value = m.place.index;
    rightOpen.value = true;
  }
}

export function SearchPanel() {
  const m = model.value;
  const opts = options.value;
  const q = query.value;
  const re = useMemo(() => compile(q, opts), [q, opts]);
  const matches = useMemo(
    () => (m && re instanceof RegExp ? searchModel(m, re) : []),
    [m?.code, m?.info, m?.widgets, re],
  );
  const [unchecked, setUnchecked] = useState<Set<string>>(new Set());
  const chosen = matches.filter((x) => !unchecked.has(x.id));
  const toggle = (k: keyof SearchOptions, label: string, title: string) => (
    <button class="icon-btn toggle" aria-pressed={opts[k]} title={title} aria-label={title}
      onClick={() => { options.value = { ...opts, [k]: !opts[k] }; }}>{label}</button>
  );

  if (!m) return <p class="empty-note">Open a model to search its code, Info and widgets.</p>;

  const replaceChosen = async () => {
    if (!(re instanceof RegExp) || chosen.length === 0) return;
    const rep = replacement.value;
    const groups = new Map<string, Match[]>();
    for (const x of chosen) {
      const key = x.place.kind === "widget" ? `w:${x.place.index}:${x.place.field}` : x.place.kind;
      groups.set(key, [...(groups.get(key) ?? []), x]);
    }
    const ops: Op[] = [];
    for (const [key, list] of groups) {
      const ranges = list.map((x) => [x.start, x.end] as [number, number]);
      if (key === "code") {
        const text = m.code;
        const changes = list.map((x) => ({ from: x.start, to: x.end, insert: replacementFor(text.slice(x.start, x.end), re, rep, opts) }));
        if (!changeOpenCode(text, changes)) await setCode(replaceRanges(text, ranges, re, rep, opts));
      } else if (key === "info") {
        await setInfo(replaceRanges(m.info, ranges, re, rep, opts));
      } else {
        const p = list[0].place;
        if (p.kind !== "widget") continue;
        const w = m.widgets[p.index];
        const text = w && fieldText(w, p.field);
        if (text === undefined) continue;
        const value = replaceRanges(text, ranges, re, rep, opts);
        if (p.field === "text") ops.push({ op: "text", index: p.index, text: value });
        else if (p.field.startsWith("part:")) ops.push({ op: "part", index: p.index, part: p.field.slice(5), text: value });
        else ops.push({ op: "attr", index: p.index, name: p.field.slice(5), value });
      }
    }
    if (ops.length) {
      // One undo step for every widget change.
      await editWidget({ op: "batch", ops }, "Replaced text in widgets");
    }
    addActivity(`Replaced ${chosen.length} match${chosen.length === 1 ? "" : "es"} of "${q}"`);
    showToast(`Replaced ${chosen.length} match${chosen.length === 1 ? "" : "es"}.`);
    setUnchecked(new Set());
  };

  // Group the results by place, in order.
  const groups: { name: string; items: Match[] }[] = [];
  for (const x of matches) {
    const name = placeName(x);
    const g = groups[groups.length - 1];
    if (g && g.name === name) g.items.push(x); else groups.push({ name, items: [x] });
  }
  const rep = replacement.value;
  const showPreview = rep !== "" || q !== "";

  return (
    <div class="search-panel">
      <div class="search-row">
        <input type="text" class="mono" placeholder="Search" aria-label="Search for" value={q} autoFocus
          onInput={(e) => { query.value = (e.target as HTMLInputElement).value; setUnchecked(new Set()); }} />
        {toggle("caseSensitive", "Aa", "Match case")}
        {toggle("wholeWord", "ab", "Whole words (NetLogo names)")}
        {toggle("regex", ".*", "Regular expression")}
      </div>
      <div class="search-row">
        <input type="text" class="mono" placeholder="Replace with" aria-label="Replace with" value={rep}
          onInput={(e) => { replacement.value = (e.target as HTMLInputElement).value; }} />
        <button class="btn small" disabled={chosen.length === 0 || !(re instanceof RegExp)} onClick={replaceChosen}
          title="Replace the checked results">Replace {chosen.length || ""}</button>
      </div>
      <p class="empty-note" role="status" style="padding: 4px 0">
        {typeof re === "string" && re ? `Not a valid expression: ${re}`
          : !q ? "Searches the code, Info and widgets of the open model."
          : matches.length === 0 ? "No results."
          : `${matches.length} result${matches.length === 1 ? "" : "s"} in ${groups.length} place${groups.length === 1 ? "" : "s"}`}
      </p>
      {rep !== "" && chosen.some((x) => x.place.kind === "widget" && x.place.field === "attr:variable") && (
        <p class="empty-note" style="padding: 0 0 4px; color: var(--warning)">
          This changes a widget's variable. Experiments that set it are not searched; to rename a variable everywhere, use Rename (F2) in the Code tab.
        </p>
      )}
      <ul class="search-results" aria-label="Search results">
        {groups.map((g) => (
          <li>
            <div class="place">{g.name}</div>
            <ul>
              {g.items.map((x) => {
                const before = x.lineText.slice(Math.max(0, x.col - 12), x.col);
                const hit = x.lineText.slice(x.col, x.col + (x.end - x.start));
                const after = x.lineText.slice(x.col + (x.end - x.start), x.col + (x.end - x.start) + 60);
                const becomes = re instanceof RegExp && rep !== "" ? replacementFor(hit, re, rep, opts) : null;
                return (
                  <li class="hit">
                    <input type="checkbox" checked={!unchecked.has(x.id)} aria-label={`Include line ${x.line} of ${g.name}`}
                      onChange={() => { const s = new Set(unchecked); if (s.has(x.id)) s.delete(x.id); else s.add(x.id); setUnchecked(s); }} />
                    <button class="hit-text mono" onClick={() => reveal(x)} title={`Line ${x.line}`}>
                      <span class="ln">{x.line}</span>
                      {x.col > 12 ? "…" : ""}{before.trimStart()}
                      <mark class={becomes !== null ? "old" : undefined}>{hit}</mark>
                      {becomes !== null && showPreview && <mark class="new">{becomes}</mark>}
                      {after}
                    </button>
                  </li>
                );
              })}
            </ul>
          </li>
        ))}
      </ul>
    </div>
  );
}

