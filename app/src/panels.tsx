// The window's side regions (design 2.2): activity bar, side panel, bottom panel and status bar.

import { useEffect, useRef, useState } from "preact/hooks";
import { Icon } from "./icons";
import {
  activity, activityView, bottomOpen, bottomTab, dirty, engine, log, model, modelTab, notApplied, outline,
  output, problems, running, ticks, type Activity, type BottomTab, runCommand,
} from "./state";
import { revealOffset } from "./code";
import { FilesPanel } from "./project";
import { ExperimentsPanel, ResultsPanel } from "./experiments";

const ACTIVITIES: { id: Activity; label: string; icon: string; later?: string }[] = [
  { id: "model", label: "Model", icon: "model" },
  { id: "files", label: "Files", icon: "files" },
  { id: "search", label: "Search", icon: "search", later: "Search and replace across the model comes in step 19." },
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
        {a.id === "model" ? <Outline /> : a.id === "files" ? <FilesPanel /> : a.id === "experiments" ? <ExperimentsPanel /> : a.id === "results" ? <ResultsPanel /> : <p class="empty-note">{a.later}</p>}
      </div>
    </aside>
  );
}

function Outline() {
  const m = model.value;
  if (!m) return <p class="empty-note">Open a model to see its globals, breeds and procedures.</p>;
  const items = outline.value;
  if (items.length === 0) return <p class="empty-note">The code has no declarations or procedures yet.</p>;
  return (
    <ul class="tree" aria-label="Outline">
      {items.map((e) => (
        <li>
          <button onClick={() => { modelTab.value = "code"; setTimeout(() => revealOffset(e.start), 30); }}
            title={`${e.name}${e.detail ? " " + e.detail : ""}, line ${e.line}`}>
            {e.kind === "procedure" || e.kind === "reporter"
              ? <><span class="kind">{e.kind === "reporter" ? "reporter" : "to"}</span><span>{e.name}</span>
                  {e.detail && <span class="detail">{e.detail}</span>}</>
              : <><span class="kind">{e.name}</span><span class="detail" style="color: var(--text)">{e.detail}</span></>}
          </button>
        </li>
      ))}
    </ul>
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
                    onClick={() => { if (p.span) { modelTab.value = "code"; setTimeout(() => revealOffset(p.span![0]), 30); } }}>
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

function CommandCenter() {
  const [agents, setAgents] = useState("observer");
  const [text, setText] = useState("");
  const [history, setHistory] = useState<string[]>([]);
  const [pos, setPos] = useState(-1);
  const out = useRef<HTMLDivElement>(null);
  useEffect(() => { const el = out.current; if (el) el.scrollTop = el.scrollHeight; }, [output.value]);
  const send = () => {
    if (!text.trim()) return;
    runCommand(text, agents);
    setHistory([text, ...history.filter((h) => h !== text)].slice(0, 100));
    setPos(-1);
    setText("");
  };
  const disabled = !model.value || engine.value.state !== "ready";
  return (
    <div class="cc">
      <div class="cc-out" ref={out} role="log" aria-label="Command Center output">
        {output.value || <span class="muted">Type NetLogo commands below. They run as {agents}, between iterations when the model is running.</span>}
      </div>
      <form class="cc-in" onSubmit={(e) => { e.preventDefault(); send(); }}>
        <label class="sr-only" for="cc-agents">Run as</label>
        <select id="cc-agents" value={agents} onChange={(e) => setAgents((e.target as HTMLSelectElement).value)}>
          <option value="observer">observer</option>
          <option value="turtles">turtles</option>
          <option value="patches">patches</option>
          <option value="links">links</option>
        </select>
        <label class="sr-only" for="cc-input">Command</label>
        <input id="cc-input" type="text" value={text} disabled={disabled} autoComplete="off" spellcheck={false}
          placeholder={disabled ? "Open a model to use the Command Center" : `${agents}>`}
          onInput={(e) => setText((e.target as HTMLInputElement).value)}
          onKeyDown={(e) => {
            if (e.key === "ArrowUp" && history.length) { const p = Math.min(pos + 1, history.length - 1); setPos(p); setText(history[p]); e.preventDefault(); }
            if (e.key === "ArrowDown") { const p = pos - 1; setPos(Math.max(p, -1)); setText(p >= 0 ? history[p] : ""); e.preventDefault(); }
          }} />
        <button class="btn small" type="submit" disabled={disabled}>Run</button>
      </form>
    </div>
  );
}

// ---- status bar ---------------------------------------------------------------------------

export function StatusBar() {
  const e = engine.value;
  const m = model.value;
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
      {m && <span>{dirty.value ? "● unsaved" : "saved"}</span>}
      {m && <span title={m.path}>{m.name}</span>}
    </footer>
  );
}
