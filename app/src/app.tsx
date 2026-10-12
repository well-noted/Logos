// The project window (design 2.1, 2.2): menus, activity bar, side panel, the model document
// with its Interface, Info and Code tabs, the right and bottom panels, and the status bar.

import { useEffect, useRef, useState } from "preact/hooks";
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
import { frames } from "./frames";
import { ipc } from "./ipc";
import {
  closeModel, dirty, model, modelTab, notApplied, paletteOpen, rightOpen, settingsOpen, toast, viewPopped,
  type ModelTab,
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
          : name === "Edit" ? [{ id: "edit.note", title: "Undo, redo and find work in the code editor", menu: "Edit" as const, enabled: () => false, run: () => {} }]
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
  return (
    <div class="split">
      <div class="doc-tabs" role="tablist" aria-label="Documents">
        <div class="doc-tab" role="tab" aria-selected="true" title={m.path}>
          {dirty.value && <span class="dot" aria-label="unsaved changes" />}
          <span>{m.name}</span>
          <button class="icon-btn" aria-label={`Close ${m.name}`} onClick={() => closeModel()}><Icon name="close" /></button>
        </div>
      </div>
      <div class="subtabs" role="tablist" aria-label="Model tabs">
        {tabs.map((t) => (
          <button role="tab" aria-selected={modelTab.value === t.id} onClick={() => { modelTab.value = t.id; }}>{t.label}</button>
        ))}
        <span class="spacer" />
        {notApplied.value && modelTab.value !== "code" && <span class="note">Code changed, not applied</span>}
        <button class="icon-btn" aria-label="Inspectors" title="Inspectors (Ctrl+I)" aria-pressed={rightOpen.value}
          onClick={() => { rightOpen.value = !rightOpen.value; }}><Icon name="panelRight" /></button>
      </div>
      {modelTab.value === "interface" && <InterfaceTab />}
      {modelTab.value === "info" && <InfoTab />}
      {modelTab.value === "code" && <CodeTab />}
    </div>
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
      {toast.value && <div class="toast" role="alert">{toast.value}</div>}
    </>
  );
}
