// The window's side regions (design 2.2): activity bar, side panel, bottom panel and status bar.

import { signal } from "@preact/signals";
import { useEffect, useRef, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "./icons";
import type { OutlineEntry } from "./ipc";
import { Compartment, EditorState } from "@codemirror/state";
import { EditorView, keymap, placeholder as placeholderExt } from "@codemirror/view";
import { defaultKeymap } from "@codemirror/commands";
import { syntaxHighlighting } from "@codemirror/language";
import { highlight } from "./code";
import { Classifier, languageNames, loadLanguageNames, modelNames, netlogoLanguage, tokenize } from "./netlogo";
import {
  activeDoc, activity, activityView, bottomOpen, bottomTab, dirty, engine, log, model, modelTab, notApplied, outline,
  ccLines, output, problems, running, ticks, type Activity, type BottomTab, runCommand,
} from "./state";
import { revealOffset } from "./code";
import { FilesPanel } from "./project";
import { ExperimentsPanel, ResultsPanel } from "./experiments";
import { SearchPanel } from "./searchpanel";

const ACTIVITIES: { id: Activity; label: string; icon: string; later?: string }[] = [
  { id: "model", label: "Model", icon: "model" },
  { id: "files", label: "Files", icon: "files" },
  { id: "search", label: "Search", icon: "search" },
  { id: "experiments", label: "Experiments", icon: "experiments" },
  { id: "results", label: "Results", icon: "results" },
  { id: "llm", label: "LLM", icon: "llm", later: "The LLM bridge editor and call log are Phase 3." },
  { id: "git", label: "Git", icon: "git", later: "The Git panel is Phase 3 (D-28)." },
];

export function ActivityBar() {
  return (
    <nav class="activitybar" aria-label="Activities">
      {ACTIVITIES.map((a) => (
        <button aria-pressed={activityView.value === a.id} aria-label={a.label} title={a.label}
          onClick={() => { activityView.value = activityView.value === a.id ? null : a.id; }}>
          <Icon name={a.icon} />
          <span>{a.label}</span>
        </button>
      ))}
    </nav>
  );
}

export function SidePanel() {
  const a = ACTIVITIES.find((x) => x.id === activityView.value);
  if (!a) return null;
  return (
    <aside class="sidepanel" aria-label={a.label}>
      <div class="panel-head"><span>{a.label}</span></div>
      <div class="panel-body">
        {a.id === "model" ? <Outline /> : a.id === "files" ? <FilesPanel /> : a.id === "experiments" ? <ExperimentsPanel /> : a.id === "results" ? <ResultsPanel /> : a.id === "search" ? <SearchPanel /> : <p class="empty-note">{a.later}</p>}
      </div>
    </aside>
  );
}

const OUTLINE_GROUPS: { kind: string; label: string }[] = [
  { kind: "extensions", label: "Extensions" },
  { kind: "globals", label: "Globals" },
  { kind: "breed", label: "Breeds" },
  { kind: "variables", label: "Agent variables" },
  { kind: "procedure", label: "Procedures" },
  { kind: "reporter", label: "Reporters" },
];
const outlineGrouped = signal(true);
const outlineFilter = signal("");

function OutlineRow({ e }: { e: OutlineEntry }) {
  return (
    <li>
      <button onClick={() => { activeDoc.value = "model"; modelTab.value = "code"; setTimeout(() => revealOffset(e.start), 30); }}
        title={`${e.name}${e.detail ? " " + e.detail : ""}, line ${e.line}`}>
        {e.kind === "procedure" || e.kind === "reporter"
          ? <><span class="kind">{e.kind === "reporter" ? "reporter" : "to"}</span><span>{e.name}</span>
              {e.detail && <span class="detail">{e.detail}</span>}</>
          : <><span class="kind">{e.name}</span><span class="detail" style="color: var(--text)">{e.detail}</span></>}
      </button>
    </li>
  );
}

function Outline() {
  const m = model.value;
  if (!m) return <p class="empty-note">Open a model to see its globals, breeds and procedures.</p>;
  const all = outline.value;
  if (all.length === 0) return <p class="empty-note">The code has no declarations or procedures yet.</p>;
  const f = outlineFilter.value.trim().toLowerCase();
  const items = f ? all.filter((e) => `${e.name} ${e.detail ?? ""}`.toLowerCase().includes(f)) : all;
  return (
    <div class="outline">
      <div class="outline-bar">
        <input type="search" placeholder="Filter" aria-label="Filter the outline" value={outlineFilter.value}
          onInput={(e) => { outlineFilter.value = (e.target as HTMLInputElement).value; }} />
        <button class="btn small" aria-pressed={outlineGrouped.value} title="Group by kind, or list in the order of the code"
          onClick={() => { outlineGrouped.value = !outlineGrouped.value; }}>{outlineGrouped.value ? "Grouped" : "In order"}</button>
      </div>
      {items.length === 0 && <p class="empty-note">Nothing matches.</p>}
      {outlineGrouped.value ? (
        OUTLINE_GROUPS.map((g) => {
          const list = items.filter((e) => e.kind === g.kind);
          if (!list.length) return null;
          return (
            <details open class="outline-group">
              <summary>{g.label} <span class="muted">{list.length}</span></summary>
              <ul class="tree" aria-label={g.label}>{list.map((e) => <OutlineRow e={e} />)}</ul>
            </details>
          );
        })
      ) : (
        <ul class="tree" aria-label="Outline">{items.map((e) => <OutlineRow e={e} />)}</ul>
      )}
    </div>
  );
}

// ---- bottom panel -------------------------------------------------------------------------

const TABS: { id: BottomTab; label: string }[] = [
  { id: "command", label: "Command Center" },
  { id: "output", label: "Output" },
  { id: "problems", label: "Problems" },
  { id: "log", label: "Log" },
  { id: "activity", label: "Activity" },
];

export function BottomPanel() {
  const open = bottomOpen.value;
  return (
    <section class={`bottompanel${open ? "" : " collapsed"}`} aria-label="Bottom panel">
      <div class="bottom-tabs" role="tablist">
        {TABS.map((t) => (
          <button class="tab" role="tab" aria-selected={open && bottomTab.value === t.id}
            onClick={() => { bottomTab.value = t.id; bottomOpen.value = true; }}>
            {t.label}{t.id === "problems" && problems.value.length ? ` (${problems.value.length})` : ""}
          </button>
        ))}
        <span class="spacer" />
        <button class="icon-btn" aria-label={open ? "Hide the bottom panel" : "Show the bottom panel"} title="Ctrl+J"
          onClick={() => { bottomOpen.value = !open; }}><Icon name="panelBottom" /></button>
      </div>
      {open && (
        <div style="flex:1;min-height:0" role="tabpanel">
          {bottomTab.value === "command" && <CommandCenter />}
          {bottomTab.value === "output" && <ScrollLog text={output.value} label="Model output" />}
          {bottomTab.value === "problems" && (
            <div class="bottom-body" role="list" aria-label="Problems">
              {problems.value.length === 0 ? <span class="muted">No problems.</span> : problems.value.map((p) => (
                <div class="list-row lvl-error" role="listitem">
                  <span class="t">{p.source}</span>
                  <button class="btn small" style="border:0;background:transparent;color:inherit;height:auto;padding:0;text-align:left"
                    onClick={() => { if (p.span) { activeDoc.value = "model"; modelTab.value = "code"; setTimeout(() => revealOffset(p.span![0]), 30); } }}>
                    {p.text}
                  </button>
                </div>
              ))}
            </div>
          )}
          {bottomTab.value === "log" && (
            <div class="bottom-body" role="log" aria-label="Log">
              {log.value.map((l) => (
                <div class={`list-row lvl-${l.level}`}><span class="t">{new Date(l.time).toLocaleTimeString()}</span><span>{l.text}</span></div>
              ))}
            </div>
          )}
          {bottomTab.value === "activity" && (
            <div class="bottom-body" role="log" aria-label="Activity">
              {activity.value.map((a) => (
                <div class="list-row"><span class="t">{new Date(a.time).toLocaleTimeString()}</span><span class="t">{a.actor}</span><span>{a.text}</span></div>
              ))}
            </div>
          )}
        </div>
      )}
    </section>
  );
}

function ScrollLog({ text, label }: { text: string; label: string }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => { const el = ref.current; if (el) el.scrollTop = el.scrollHeight; }, [text]);
  return <div class="bottom-body" ref={ref} role="log" aria-label={label}>{text || <span class="muted">Nothing printed yet.</span>}</div>;
}

/** One line of NetLogo with the Code tab's colors. */
function Highlighted({ text, c }: { text: string; c: Classifier }) {
  return <>{tokenize(text, c).map((tk) => (tk.cls ? <span class={`syn-${tk.cls}`}>{tk.text}</span> : tk.text))}</>;
}

function CommandCenter() {
  const [agents, setAgents] = useState("observer");
  const history = useRef<string[]>([]);
  const pos = useRef(-1);
  const out = useRef<HTMLDivElement>(null);
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const agentsRef = useRef(agents);
  agentsRef.current = agents;
  const lines = ccLines.value;
  const globals = (model.value?.widgets ?? []).map((w) => ("variable" in w.data ? w.data.variable : "")).filter(Boolean);
  const classifier = new Classifier(languageNames.value, modelNames(outline.value, globals));
  useEffect(() => { const el = out.current; if (el) el.scrollTop = el.scrollHeight; }, [lines]);
  useEffect(() => { loadLanguageNames(); }, [model.value?.id, engine.value.state]);
  const disabled = !model.value || engine.value.state !== "ready";

  // A one-line CodeMirror editor, so the command is colored as in the Code tab.
  useEffect(() => {
    if (!host.current) return;
    const setDoc = (v: EditorView, text: string) => v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: text }, selection: { anchor: text.length } });
    const send = (v: EditorView) => {
      const text = v.state.doc.toString();
      if (!text.trim()) return true;
      runCommand(text, agentsRef.current);
      history.current = [text, ...history.current.filter((h) => h !== text)].slice(0, 100);
      pos.current = -1;
      setDoc(v, "");
      return true;
    };
    const v = new EditorView({
      parent: host.current,
      state: EditorState.create({
        doc: "",
        extensions: [
          ccLanguage.of(netlogoLanguage(new Classifier(languageNames.value, null))),
          syntaxHighlighting(highlight),
          ccEditable.of(EditorView.editable.of(true)),
          EditorState.transactionFilter.of((tr) => (tr.newDoc.lines > 1 ? [] : tr)),
          EditorView.contentAttributes.of({ "aria-label": "Command", id: "cc-input" }),
          placeholderExt(`Type a NetLogo command and press Enter`),
          keymap.of([
            { key: "Enter", run: send },
            { key: "ArrowUp", run: (ed) => { if (!history.current.length) return true; pos.current = Math.min(pos.current + 1, history.current.length - 1); setDoc(ed, history.current[pos.current]); return true; } },
            { key: "ArrowDown", run: (ed) => { pos.current = Math.max(pos.current - 1, -1); setDoc(ed, pos.current >= 0 ? history.current[pos.current] : ""); return true; } },
            ...defaultKeymap,
          ]),
        ],
      }),
    });
    view.current = v;
    return () => { v.destroy(); view.current = null; };
  }, []);
  useEffect(() => {
    view.current?.dispatch({ effects: ccLanguage.reconfigure(netlogoLanguage(classifier)) });
  }, [languageNames.value, outline.value, model.value?.widgets]);
  useEffect(() => {
    view.current?.dispatch({ effects: ccEditable.reconfigure(EditorView.editable.of(!disabled)) });
  }, [disabled]);

  return (
    <div class="cc">
      <div class="cc-out mono" ref={out} role="log" aria-label="Command Center output">
        {lines.length === 0 && <span class="muted">Type NetLogo commands below. They run as {agents}, between iterations when the model is running.</span>}
        {lines.map((l) => l.kind === "input"
          ? <div class="cc-line cc-input"><span class="muted">{l.agents}&gt; </span><Highlighted text={l.text} c={classifier} /></div>
          : l.kind === "error"
            ? <div class="cc-line cc-error" role="alert">{l.text}</div>
            : <div class="cc-line cc-printed">{l.text.replace(/\n$/, "")}</div>)}
      </div>
      <form class="cc-in" onSubmit={(e) => e.preventDefault()}>
        <label class="sr-only" for="cc-agents">Run as</label>
        <select id="cc-agents" value={agents} onChange={(e) => setAgents((e.target as HTMLSelectElement).value)}>
          <option value="observer">observer</option>
          <option value="turtles">turtles</option>
          <option value="patches">patches</option>
          <option value="links">links</option>
        </select>
        <div class={`cc-editor${disabled ? " disabled" : ""}`} ref={host} />
        <button class="btn small" type="button" disabled={disabled}
          onClick={() => { const v = view.current; if (!v) return; const text = v.state.doc.toString(); if (!text.trim()) return;
            runCommand(text, agents); history.current = [text, ...history.current.filter((h) => h !== text)].slice(0, 100);
            v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: "" } }); v.focus(); }}>Run</button>
      </form>
    </div>
  );
}

const ccLanguage = new Compartment();
const ccEditable = new Compartment();

// ---- status bar ---------------------------------------------------------------------------

interface GitStatus { branch: string; changed: number }

/** The git branch and changed-file count, refreshed when the model or its saved state
 * changes and every 15 seconds (spec part R). */
function useGitStatus(id: number | undefined, saved: boolean): GitStatus | null {
  const [g, setG] = useState<GitStatus | null>(null);
  useEffect(() => {
    if (id === undefined) { setG(null); return; }
    let live = true;
    const load = () => invoke<GitStatus | null>("git_status", { id }).then((r) => { if (live) setG(r); }).catch(() => {});
    load();
    const t = window.setInterval(load, 15000);
    return () => { live = false; clearInterval(t); };
  }, [id, saved]);
  return g;
}

export function StatusBar() {
  const e = engine.value;
  const m = model.value;
  const git = useGitStatus(m?.id, !dirty.value);
  const engineText = e.state === "ready"
    ? `NetLogo ${e.hello?.netlogoVersion?.replace("NetLogo ", "") ?? e.version}`
    : { searching: "Looking for NetLogo", notFound: "NetLogo not found", starting: "Starting NetLogo", failed: "Engine failed", ready: "" }[e.state];
  const state = !m ? "" : running.value ? "running" : m.compiled ? "compiled" : "compile errors";
  return (
    <footer class="statusbar" aria-label="Status">
      <span><span class={`state-dot${e.state === "ready" ? " on" : ""}`} aria-hidden="true" />{engineText}</span>
      {m && <span>{state}</span>}
      {m && <span>ticks {ticks.value ?? "–"}</span>}
      {notApplied.value && <span>Changes not applied</span>}
      <span class="grow" />
      {problems.value.length > 0 && <span>{problems.value.length} problem{problems.value.length > 1 ? "s" : ""}</span>}
      {git && <span title={`${git.changed} changed file${git.changed === 1 ? "" : "s"} in the repository`}>
        <Icon name="git" /> {git.branch}{git.changed ? ` · ${git.changed} changed` : ""}</span>}
      {m && <span>{dirty.value ? "● unsaved" : "saved"}</span>}
      {m && <span title={m.path}>{m.name}</span>}
    </footer>
  );
}
