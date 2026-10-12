// Comparing the open model's code with the saved file, the version committed in git, or
// another model file. Read-only; edits happen in the Code tab.

import { useEffect, useRef, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { EditorState } from "@codemirror/state";
import { EditorView, lineNumbers } from "@codemirror/view";
import { syntaxHighlighting } from "@codemirror/language";
import { MergeView } from "@codemirror/merge";
import { errorText } from "./ipc";
import { highlight } from "./code";
import { Classifier, languageNames, modelNames, netlogoLanguage } from "./netlogo";
import { docs, model, openDoc, outline, showToast } from "./state";

export type CompareSource = { kind: "saved" } | { kind: "head" } | { kind: "file"; path: string };

/** Opens a comparison document for the open model's code. */
export async function compareCode(source: CompareSource | "pick") {
  const m = model.value;
  if (!m) return;
  let src: CompareSource;
  if (source === "pick") {
    const path = await openDialog({ multiple: false, filters: [{ name: "NetLogo models", extensions: ["nlogox", "nlogo"] }] });
    if (typeof path !== "string") return;
    src = { kind: "file", path };
  } else src = source;
  try {
    const r = await invoke<{ label: string; code: string }>("model_compare_source", { id: m.id, source: src });
    const id = `diff-${r.label}`;
    docs.value = docs.value.filter((d) => d.id !== id);
    openDoc({ id, kind: "diff", label: r.label, code: r.code });
  } catch (e) {
    showToast(errorText(e));
  }
}

export function DiffDoc({ label, code }: { label: string; code: string }) {
  const host = useRef<HTMLDivElement>(null);
  const m = model.value;
  const current = m?.code ?? "";
  const [stats, setStats] = useState<{ chunks: number } | null>(null);
  useEffect(() => {
    if (!host.current) return;
    const globals = (m?.widgets ?? []).map((w) => ("variable" in w.data ? w.data.variable : "")).filter(Boolean);
    const lang = netlogoLanguage(new Classifier(languageNames.value, modelNames(outline.value, globals)));
    const ext = [
      lineNumbers(), lang, syntaxHighlighting(highlight), EditorState.readOnly.of(true), EditorView.editable.of(false),
      EditorView.theme({ "&": { backgroundColor: "var(--code-bg)", color: "var(--code-text)", height: "100%" } }),
    ];
    const view = new MergeView({
      a: { doc: code, extensions: [...ext, EditorView.contentAttributes.of({ "aria-label": `Code of ${label}` })] },
      b: { doc: current, extensions: [...ext, EditorView.contentAttributes.of({ "aria-label": "Code of the open model" })] },
      parent: host.current,
      collapseUnchanged: { margin: 3, minSize: 6 },
      gutter: true,
    });
    setStats({ chunks: view.chunks.length });
    return () => view.destroy();
  }, [code, current, languageNames.value]);
  return (
    <div class="split">
      <div class="code-bar" role="toolbar" aria-label="Comparison">
        <strong>{label}</strong><span class="muted">compared with the open model's code</span>
        <span style="flex: 1" />
        <span class="muted" role="status">{stats ? (stats.chunks === 0 ? "No differences" : `${stats.chunks} changed part${stats.chunks === 1 ? "" : "s"}`) : ""}</span>
      </div>
      <div class="diff-host" ref={host} />
    </div>
  );
}
