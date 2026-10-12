// Every action in the app, for the command palette, the menus and keyboard shortcuts
// (design principle 7). Shortcuts are stored per action so they can be rebound later.

import {
  activityView, applyCode, bottomOpen, bottomTab, closeModel, model, modelTab, openModelDialog,
  paletteOpen, rightOpen, running, saveModel, settingsOpen, stepModel, stopModel, viewPopped, engine,
} from "./state";
import { canRedo, canUndo, editMode, undoInterface } from "./editor";

export interface Command {
  id: string;
  title: string;
  menu: "File" | "Edit" | "View" | "Model" | "Run" | "Tools" | "Help";
  key?: string;
  enabled?: () => boolean;
  /** False: the key is left to text fields and the code editor when one has focus. */
  textKey?: boolean;
  run: () => void | Promise<void>;
}

const hasModel = () => model.value !== null;

export const COMMANDS: Command[] = [
  { id: "file.open", title: "Open model…", menu: "File", key: "Ctrl+O", run: () => openModelDialog() },
  { id: "file.openExample", title: "Open a Models Library example…", menu: "File",
    enabled: () => !!engine.value.hello?.modelsDir, run: () => openModelDialog(engine.value.hello?.modelsDir ?? undefined) },
  { id: "file.save", title: "Save", menu: "File", key: "Ctrl+S", enabled: hasModel, run: () => saveModel() },
  { id: "file.close", title: "Close model", menu: "File", key: "Ctrl+W", enabled: hasModel, run: () => { closeModel(); } },
  { id: "file.settings", title: "Settings and themes…", menu: "File", key: "Ctrl+,", run: () => { settingsOpen.value = true; } },
  { id: "view.palette", title: "Show all commands", menu: "View", key: "Ctrl+Shift+P", run: () => { paletteOpen.value = true; } },
  { id: "view.interface", title: "Go to Interface tab", menu: "View", key: "Ctrl+1", enabled: hasModel, run: () => { modelTab.value = "interface"; } },
  { id: "view.info", title: "Go to Info tab", menu: "View", key: "Ctrl+2", enabled: hasModel, run: () => { modelTab.value = "info"; } },
  { id: "view.code", title: "Go to Code tab", menu: "View", key: "Ctrl+3", enabled: hasModel, run: () => { modelTab.value = "code"; } },
  { id: "view.outline", title: "Show model outline", menu: "View", key: "Ctrl+Shift+O",
    run: () => { activityView.value = activityView.value === "model" ? null : "model"; } },
  { id: "view.search", title: "Search and replace in the model", menu: "Edit", key: "Ctrl+Shift+F",
    run: () => { activityView.value = "search"; setTimeout(() => (document.querySelector(".search-panel input") as HTMLInputElement | null)?.focus(), 0); } },
  { id: "view.bottom", title: "Show or hide the bottom panel", menu: "View", key: "Ctrl+J", run: () => { bottomOpen.value = !bottomOpen.value; } },
  { id: "view.commandCenter", title: "Go to Command Center", menu: "View", key: "Ctrl+`",
    run: () => { bottomOpen.value = true; bottomTab.value = "command"; setTimeout(() => document.getElementById("cc-input")?.focus(), 0); } },
  { id: "view.inspector", title: "Show or hide inspectors", menu: "View", key: "Ctrl+I", run: () => { rightOpen.value = !rightOpen.value; } },
  { id: "view.popout", title: "Pop out view", menu: "View", key: "Ctrl+Shift+V", enabled: hasModel, run: () => { viewPopped.value = !viewPopped.value; } },
  { id: "view.layoutNetLogo", title: "Layout: NetLogo", menu: "View", enabled: hasModel,
    run: () => { modelTab.value = "interface"; viewPopped.value = false; rightOpen.value = false; bottomOpen.value = true; bottomTab.value = "command"; } },
  { id: "view.layoutCodeRun", title: "Layout: Code and run", menu: "View", enabled: hasModel,
    run: () => { modelTab.value = "code"; viewPopped.value = true; rightOpen.value = true; } },
  { id: "edit.undo", title: "Undo interface change", menu: "Edit", key: "Ctrl+Z", textKey: false,
    enabled: () => editMode.value && canUndo.value, run: () => undoInterface(false) },
  { id: "edit.redo", title: "Redo interface change", menu: "Edit", key: "Ctrl+Y", textKey: false,
    enabled: () => editMode.value && canRedo.value, run: () => undoInterface(true) },
  { id: "edit.toggle", title: "Edit the interface", menu: "Edit", key: "Ctrl+E", enabled: hasModel,
    run: () => { modelTab.value = "interface"; editMode.value = !editMode.value; } },
  { id: "model.apply", title: "Apply code changes", menu: "Model", key: "Ctrl+Shift+Enter", enabled: hasModel, run: () => applyCode() },
  { id: "run.step", title: "Step one tick", menu: "Run", key: "F10", enabled: () => hasModel() && !running.value, run: () => stepModel() },
  { id: "run.pause", title: "Pause", menu: "Run", key: "F6", enabled: () => !!running.value, run: () => stopModel() },
];

function normalize(e: KeyboardEvent): string {
  const parts: string[] = [];
  if (e.ctrlKey || e.metaKey) parts.push("Ctrl");
  if (e.shiftKey) parts.push("Shift");
  if (e.altKey) parts.push("Alt");
  let k = e.key;
  if (k === " ") k = "Space";
  if (k.length === 1) k = k.toUpperCase();
  if (["Control", "Shift", "Alt", "Meta"].includes(k)) return "";
  parts.push(k);
  return parts.join("+");
}

export function runCommand(c: Command) {
  if (c.enabled && !c.enabled()) return;
  void c.run();
}

export function installShortcuts() {
  window.addEventListener("keydown", (e) => {
    const k = normalize(e);
    if (!k) return;
    const c = COMMANDS.find((c) => c.key === k);
    if (!c) return;
    // Text fields and the code editor keep their own keys for undo and the like.
    const t = e.target as HTMLElement | null;
    const editable = !!t && (t.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName));
    if (c.textKey === false && editable) return;
    if (c.enabled && !c.enabled()) return;
    e.preventDefault();
    runCommand(c);
  });
}
