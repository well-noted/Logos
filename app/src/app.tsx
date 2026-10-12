// The project window (design 2.1, 2.2): menus, activity bar, side panel, the model document
// with its Interface, Info and Code tabs, the right and bottom panels, and the status bar.

import { useEffect, useRef, useState } from "preact/hooks";
import { ContextMenu, type MenuItem, isMenuKey, showContextMenu } from "./menu";
import { DiffDoc, compareCode } from "./diff";
import { ConfirmDialog } from "./dialog";
import { RecoveryDialog } from "./recovery";
import { COMMANDS, runCommand, type Command } from "./commands";
import { Icon } from "./icons";
import { InterfaceTab, PopOutView } from "./interface";
import { InfoTab } from "./info";
import { CodeTab } from "./code";
import { RightPanel } from "./inspector";
import { ActivityBar, BottomPanel, SidePanel, StatusBar } from "./panels";
import { Palette } from "./palette";
import { SettingsDialog } from "./settings";
import { Launcher } from "./launcher";
import { ProjectDoc, SecretsDoc } from "./project";
import { DashboardDoc, ExperimentDoc } from "./experiments";
import { DataDoc } from "./data";
import { frames } from "./frames";
import { ipc } from "./ipc";
import {
  activeDoc, closeDoc, closeModel, dirty, docs, model, modelTab, notApplied, paletteOpen, rightOpen, settingsOpen, sideDoc,
  toast, viewPopped, type Doc, type ModelTab,
} from "./state";

const MENUS: Command["menu"][] = ["File", "Edit", "View", "Model", "Run", "Tools", "Help"];

function Menus() {
  const [open, setOpen] = useState<string | null>(null);
  const bar = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const close = (e: MouseEvent) => { if (!bar.current?.contains(e.target as Node)) setOpen(null); };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, []);
  return (
    <div class="menubar" role="menubar" ref={bar}>
      {MENUS.map((name) => {
        const items = COMMANDS.filter((c) => c.menu === name);
        const extra: Command[] = name === "Help"
          ? [{ id: "help.palette", title: "All commands…", menu: "Help" as const, key: "Ctrl+Shift+P", run: () => { paletteOpen.value = true; } }]
          : name === "Edit" ? [{ id: "edit.note", title: "In the code editor, Ctrl+Z, Ctrl+Y and Ctrl+F work on the code", menu: "Edit" as const, enabled: () => false, run: () => {} }]
          : name === "Tools" ? [{ id: "tools.settings", title: "Settings and themes…", menu: "Tools" as const, run: () => { settingsOpen.value = true; } }]
          : [];
        const all = [...items, ...extra];
        return (
          <div class="menu">
            <button role="menuitem" aria-haspopup="true" aria-expanded={open === name}
              onClick={() => setOpen(open === name ? null : name)}
              onKeyDown={(e) => { if (e.key === "Escape") setOpen(null); }}>{name}</button>
            {open === name && (
              <ul class="menu-list" role="menu" aria-label={name}>
                {all.map((c) => (
                  <li role="none">
                    <button role="menuitem" disabled={c.enabled ? !c.enabled() : false}
                      onClick={() => { setOpen(null); runCommand(c); }}
                      onKeyDown={(e) => { if (e.key === "Escape") setOpen(null); }}>
                      <span>{c.title}</span>{c.key && <span class="kbd">{c.key}</span>}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        );
      })}
    </div>
  );
}

function docTitle(d: Doc): string {
  switch (d.kind) {
    case "model": return model.value?.name ?? "Model";
    case "project": return "Project";
    case "secrets": return "Secrets";
    case "experiment": return `Experiment ${d.name}`;
    case "dashboard": return `Run: ${d.name}`;
    case "data": return d.path.split("/").pop() ?? d.path;
    case "diff": return `Compare: ${d.label}`;
  }
}

function tabMenu(e: MouseEvent | KeyboardEvent, d: Doc) {
  const items: MenuItem[] = [];
  const isSide = sideDoc.value === d.id;
  const isActive = activeDoc.value === d.id;
  if (isSide) items.push({ label: "Close the side view", run: () => { sideDoc.value = null; } });
  else if (!isActive) items.push({ label: "Open to the side", run: () => { sideDoc.value = d.id; } });
  else {
    const other = docs.value.find((x) => x.id !== d.id && x.id !== sideDoc.value);
    items.push({
      label: "Open to the side", disabled: !other,
      run: () => { sideDoc.value = d.id; if (other) activeDoc.value = other.id; },
    });
  }
  if (sideDoc.value && !isSide) items.push({ label: "Swap sides", run: () => { const s = sideDoc.value!; sideDoc.value = activeDoc.value; activeDoc.value = s; } });
  if (d.kind === "model") {
    items.push("separator",
      { label: "Compare code with the saved file", run: () => void compareCode({ kind: "saved" }) },
      { label: "Compare code with git HEAD", run: () => void compareCode({ kind: "head" }) },
      { label: "Compare code with another model…", run: () => void compareCode("pick") });
  }
  items.push("separator", { label: "Close", run: () => { if (d.kind === "model") closeModel(); else closeDoc(d.id); } });
  showContextMenu(e, `${docTitle(d)} tab`, items);
}

function DocTabs() {
  const m = model.value!;
  return (
    <div class="doc-tabs" role="tablist" aria-label="Documents">
      {docs.value.map((d) => {
        const active = activeDoc.value === d.id;
        return (
          <div class={`doc-tab${active ? "" : " inactive"}${sideDoc.value === d.id ? " side" : ""}`} role="tab" aria-selected={active} tabIndex={0}
            title={d.kind === "model" ? m.path : docTitle(d)}
            onClick={() => { if (sideDoc.value === d.id) return; activeDoc.value = d.id; }}
            onContextMenu={(e) => tabMenu(e, d)}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") activeDoc.value = d.id;
              if (isMenuKey(e)) tabMenu(e, d);
            }}>
            {d.kind === "model" && dirty.value && <span class="dot" aria-label="unsaved changes" />}
            <span>{docTitle(d)}</span>
            <button class="icon-btn close" aria-label={`Close ${docTitle(d)}`}
              onClick={(e) => { e.stopPropagation(); if (d.kind === "model") closeModel(); else closeDoc(d.id); }}>
              <Icon name="close" />
            </button>
          </div>
        );
      })}
    </div>
  );
}

function ModelDocument() {
  const m = model.value;
  const viewRef = useRef<number | null>(null);
  // Attach the view stream once per model.
  useEffect(() => {
    if (!m) return;
    if (viewRef.current === m.id) return;
    viewRef.current = m.id;
    frames.reset();
    ipc.attachView(m.id, frames.receive).catch(() => {});
  }, [m?.id]);
  if (!m) return null;
  const tabs: { id: ModelTab; label: string }[] = [
    { id: "interface", label: "Interface" }, { id: "info", label: "Info" }, { id: "code", label: "Code" },
  ];
  const doc = docs.value.find((d) => d.id === activeDoc.value) ?? docs.value[0];
  const side = docs.value.find((d) => d.id === sideDoc.value && d.id !== doc?.id);
  return (
    <div class="split">
      <DocTabs />
      <div class={side ? "doc-split" : "doc-single"}>
        <div class="doc-pane" aria-label={doc ? docTitle(doc) : "Document"}>{doc ? <DocContent doc={doc} tabs={tabs} /> : null}</div>
        {side && (
          <div class="doc-pane side" aria-label={`${docTitle(side)}, beside`}>
            <div class="side-head"><span>{docTitle(side)}</span>
              <button class="icon-btn" aria-label="Close the side view" onClick={() => { sideDoc.value = null; }}><Icon name="close" /></button></div>
            {side.kind === "model" && doc?.kind === "model" ? null : <DocContent doc={side} tabs={tabs} />}
          </div>
        )}
      </div>
    </div>
  );
}

function DocContent({ doc, tabs }: { doc: Doc; tabs: { id: ModelTab; label: string }[] }) {
  return (
    <>
      {doc.kind === "model" ? (
        <>
          <div class="subtabs" role="tablist" aria-label="Model tabs">
            {tabs.map((t) => (
              <button role="tab" aria-selected={modelTab.value === t.id} onClick={() => { modelTab.value = t.id; }}>{t.label}</button>
            ))}
            <span class="spacer" />
            {notApplied.value && modelTab.value !== "code" && <span class="note">Changes not applied</span>}
            <button class="icon-btn" aria-label="Inspectors" title="Inspectors (Ctrl+I)" aria-pressed={rightOpen.value}
              onClick={() => { rightOpen.value = !rightOpen.value; }}><Icon name="panelRight" /></button>
          </div>
          {modelTab.value === "interface" && <InterfaceTab />}
          {modelTab.value === "info" && <InfoTab />}
          {modelTab.value === "code" && <CodeTab />}
        </>
      ) : doc.kind === "project" ? <ProjectDoc />
        : doc.kind === "secrets" ? <SecretsDoc />
        : doc.kind === "experiment" ? <ExperimentDoc name={doc.name} docId={doc.id} />
        : doc.kind === "data" ? <DataDoc path={doc.path} />
        : doc.kind === "diff" ? <DiffDoc label={doc.label} code={doc.code} />
        : <DashboardDoc runId={doc.runId} dir={doc.dir} />}
    </>
  );
}

export function App() {
  const m = model.value;
  return (
    <>
      <div class="window">
        <header class="titlebar">
          <Menus />
          <span class="title">{m ? `${m.name}${dirty.value ? " ●" : ""}` : "Logos"}</span>
          <button class="icon-btn" aria-label="All commands" title="All commands (Ctrl+Shift+P)" onClick={() => { paletteOpen.value = true; }}>
            <Icon name="palette" />
          </button>
        </header>
        <div class="main">
          <ActivityBar />
          <SidePanel />
          <div class="center">
            <div class="editor-area">
              {m ? <ModelDocument /> : <Launcher />}
              {m && rightOpen.value && <RightPanel />}
            </div>
            {m && <BottomPanel />}
          </div>
        </div>
        <StatusBar />
      </div>
      {m && viewPopped.value && <PopOutView />}
      {paletteOpen.value && <Palette />}
      {settingsOpen.value && <SettingsDialog />}
      <RecoveryDialog />
      <ConfirmDialog />
      <ContextMenu />
      {toast.value && <div class="toast" role="alert">{toast.value}</div>}
    </>
  );
}
