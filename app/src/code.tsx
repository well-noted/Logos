// The Code tab (design 2.4, 4.2): CodeMirror 6 with NetLogo highlighting, NetLogo's compile
// errors as underlines, the outline, and Apply (D-23).

import { useEffect, useRef } from "preact/hooks";
import { EditorState, type Extension } from "@codemirror/state";
import {
  EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter, drawSelection,
  highlightSpecialChars, rectangularSelection,
} from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { searchKeymap, highlightSelectionMatches } from "@codemirror/search";
import {
  HighlightStyle, syntaxHighlighting, bracketMatching, indentOnInput, foldGutter, foldKeymap,
} from "@codemirror/language";
import { closeBrackets, closeBracketsKeymap, autocompletion, completionKeymap } from "@codemirror/autocomplete";
import { lintGutter } from "@codemirror/lint";
import { invoke } from "@tauri-apps/api/core";
import { lspClient } from "./lsp";
import { jumpToDefinition } from "@codemirror/lsp-client";
import { tags as t } from "@lezer/highlight";
import { Compartment } from "@codemirror/state";
import {
  Classifier, languageNames, loadLanguageNames, modelNames, netlogoLanguage, tagCommand, tagReporter,
  tagUserProcedure, tagVariable,
} from "./netlogo";
import { applyCode, dirty, model, notApplied, outline, setCode } from "./state";
import { Icon } from "./icons";

export const highlight = HighlightStyle.define([
  { tag: t.keyword, color: "var(--syn-keyword)", fontWeight: "600" },
  { tag: tagCommand, color: "var(--syn-command)" },
  { tag: tagReporter, color: "var(--syn-reporter)" },
  { tag: tagUserProcedure, color: "var(--syn-procedure)" },
  { tag: tagVariable, color: "var(--syn-variable)" },
  { tag: t.comment, color: "var(--syn-comment)", fontStyle: "italic" },
  { tag: t.string, color: "var(--syn-string)" },
  { tag: t.number, color: "var(--syn-number)" },
  { tag: t.definition(t.variableName), color: "var(--syn-procedure)", fontWeight: "600" },
  { tag: [t.bracket, t.squareBracket, t.paren], color: "var(--syn-bracket)" },
]);

const theme = EditorView.theme({
  "&": { color: "var(--code-text)", backgroundColor: "var(--code-bg)", height: "100%" },
  ".cm-content": { caretColor: "var(--code-text)" },
  ".cm-cursor": { borderLeftColor: "var(--code-text)" },
  ".cm-gutters": { backgroundColor: "var(--code-bg)", color: "var(--code-gutter)", borderRight: "1px solid var(--border)" },
  // Translucent, so the selection drawn under the text stays visible on the current line.
  ".cm-activeLine": { backgroundColor: "color-mix(in srgb, var(--bg-hover) 55%, transparent)" },
  ".cm-selectionMatch": { backgroundColor: "color-mix(in srgb, var(--selection) 70%, transparent)", outline: "1px solid var(--focus)" },
  ".cm-searchMatch": { backgroundColor: "color-mix(in srgb, var(--warning) 30%, transparent)", outline: "1px solid var(--warning)" },
  ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: "var(--selection)" },
  ".cm-activeLineGutter": { backgroundColor: "var(--bg-hover)" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": { backgroundColor: "var(--selection) !important" },
  ".cm-panel input, .cm-panel button, .cm-textfield": { color: "var(--text)", backgroundColor: "var(--bg-input)", border: "1px solid var(--control-border)" },
  ".cm-tooltip": { backgroundColor: "var(--bg-panel)", color: "var(--text)", border: "1px solid var(--control-border)" },
  ".cm-panels": { backgroundColor: "var(--bg-panel)", color: "var(--text)" },
  ".cm-diagnostic-error": { borderLeft: "4px solid var(--error)" },
  ".cm-lintRange-error": { backgroundImage: "none", textDecoration: "underline wavy var(--error)", textUnderlineOffset: "3px" },
});

const language = new Compartment();

/** The open editor, so the outline can move the cursor. */
let current: EditorView | null = null;

export function revealOffset(offset: number) {
  if (!current) return;
  const pos = Math.min(offset, current.state.doc.length);
  current.dispatch({ selection: { anchor: pos }, scrollIntoView: true });
  current.focus();
}

/** Puts new text in the open editor (a reload or a restore); its undo can take it back. */
export function replaceOpenCode(text: string) {
  if (!current || current.state.doc.toString() === text) return;
  current.dispatch({ changes: { from: 0, to: current.state.doc.length, insert: text } });
}

/** Selects a range in the open editor, as search does. */
export function revealRange(start: number, end: number) {
  if (!current) return;
  const len = current.state.doc.length;
  current.dispatch({ selection: { anchor: Math.min(start, len), head: Math.min(end, len) }, scrollIntoView: true });
  current.focus();
}

/** Applies replacements to the open editor, so the editor's own undo covers them. False
 * when the Code tab isn't open; the caller then sets the code directly. */
export function changeOpenCode(expected: string, changes: { from: number; to: number; insert: string }[]): boolean {
  if (!current || current.state.doc.toString() !== expected) return false;
  current.dispatch({ changes, userEvent: "input.replace" });
  return true;
}

/** The names that decide colors: NetLogo's primitive table and the model's declarations. */
function currentClassifier(): Classifier {
  const m = model.value;
  const globals = (m?.widgets ?? []).map((w) => ("variable" in w.data ? w.data.variable : "")).filter(Boolean);
  return new Classifier(languageNames.value, modelNames(outline.value, globals));
}

export function CodeTab() {
  const host = useRef<HTMLDivElement>(null);
  const m = model.value;
  useEffect(() => {
    if (!host.current || !m) return;
    let timer = 0;
    let view: EditorView | null = null;
    let cancelled = false;
    loadLanguageNames();
    invoke<string>("lsp_code_uri", { id: m.id }).catch(() => null).then((uri) => {
      if (cancelled || !host.current) return;
      const lsp: Extension[] = uri ? [lspClient().plugin(uri, "netlogo")] : [];
      view = new EditorView({
      parent: host.current,
      state: EditorState.create({
        doc: m.code,
        extensions: [
          lineNumbers(), highlightActiveLineGutter(), highlightSpecialChars(), history(), foldGutter(),
          drawSelection(), indentOnInput(), bracketMatching(), closeBrackets(), autocompletion(),
          rectangularSelection(), highlightActiveLine(), highlightSelectionMatches({ wholeWords: true, minSelectionLength: 1 }), lintGutter(),
          language.of(netlogoLanguage(currentClassifier())), syntaxHighlighting(highlight), theme, ...lsp,
          // Ctrl+click (Cmd+click on macOS) goes to the definition, as F12 does.
          EditorView.domEventHandlers({
            mousedown(e, v) {
              if (!(e.ctrlKey || e.metaKey) || e.button !== 0) return false;
              const pos = v.posAtCoords({ x: e.clientX, y: e.clientY });
              if (pos === null) return false;
              e.preventDefault();
              v.dispatch({ selection: { anchor: pos } });
              jumpToDefinition(v);
              return true;
            },
          }),
          EditorView.contentAttributes.of({ "aria-label": "Model code" }),
          keymap.of([
            ...closeBracketsKeymap, ...defaultKeymap, ...searchKeymap, ...historyKeymap, ...foldKeymap,
            ...completionKeymap, indentWithTab,
          ]),
          EditorView.updateListener.of((u) => {
            if (!u.docChanged) return;
            clearTimeout(timer);
            const text = u.state.doc.toString();
            timer = window.setTimeout(() => setCode(text), 250);
          }),
        ],
      }),
    });
      current = view;
    });
    return () => {
      cancelled = true;
      clearTimeout(timer);
      if (view) { if (current === view) current = null; view.destroy(); }
    };
  }, [m?.id, m?.path]);
  // Recolor when the primitive table arrives or the model's declarations change.
  const names = languageNames.value;
  const decls = JSON.stringify(outline.value.filter((e) => e.kind !== "procedure" && e.kind !== "reporter").map((e) => e.detail))
    + outline.value.filter((e) => e.kind === "procedure" || e.kind === "reporter").map((e) => e.name).join(" ");
  useEffect(() => {
    if (current) current.dispatch({ effects: language.reconfigure(netlogoLanguage(currentClassifier())) });
  }, [names, decls]);
  return (
    <div class="split">
      <div class="code-bar" role="toolbar" aria-label="Code">
        <button class="btn small primary" onClick={applyCode} title="Apply code changes to the model (Ctrl+Shift+Enter)">
          <Icon name="check" /> Apply
        </button>
        <span class="state" role="status">
          {notApplied.value ? <span style="color: var(--warning)">Changes not applied</span> : <span class="muted">Applied</span>}
          {dirty.value ? <span class="muted"> · unsaved</span> : null}
        </span>
        <span style="flex:1" />
        <span class="muted" style="font-size: 12px">
          NetLogo checks the code as you type. Ctrl+click or F12 goes to a definition, F2 renames, Shift+F12 finds references.
        </span>
      </div>
      <div class="code-host" ref={host} />
    </div>
  );
}
