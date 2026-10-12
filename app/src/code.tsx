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
  StreamLanguage, HighlightStyle, syntaxHighlighting, bracketMatching, indentOnInput, foldGutter, foldKeymap,
} from "@codemirror/language";
import { closeBrackets, closeBracketsKeymap, autocompletion, completionKeymap } from "@codemirror/autocomplete";
import { lintGutter } from "@codemirror/lint";
import { invoke } from "@tauri-apps/api/core";
import { lspClient } from "./lsp";
import { tags as t } from "@lezer/highlight";
import { applyCode, dirty, model, notApplied, setCode } from "./state";
import { Icon } from "./icons";

const DECLARATIONS = new Set([
  "globals", "breed", "directed-link-breed", "undirected-link-breed", "extensions", "__includes",
  "turtles-own", "patches-own", "links-own",
]);
const KEYWORDS = new Set([
  "to", "to-report", "end", "ask", "ask-concurrent", "if", "ifelse", "ifelse-value", "let", "set", "report", "stop",
  "while", "repeat", "loop", "foreach", "map", "filter", "reduce", "with", "of", "and", "or", "not", "xor",
  "true", "false", "nobody", "tick", "reset-ticks", "clear-all", "ca", "carefully", "run", "runresult",
  "every", "wait", "without-interruption", "hatch", "sprout", "die", "create-turtles", "crt",
]);

interface NlState { afterTo: boolean }

/** A tokenizer for highlighting only; the language server's parser (D-11) is the real one. */
const netlogo = StreamLanguage.define<NlState>({
  name: "netlogo",
  startState: () => ({ afterTo: false }),
  token(stream, state) {
    if (stream.eatSpace()) return null;
    if (stream.match(";")) { stream.skipToEnd(); return "comment"; }
    if (stream.match(/^"(?:[^"\\]|\\.)*"?/)) return "string";
    if (stream.match(/^-?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?(?![\w\-?!.])/)) return "number";
    if (stream.match(/^[[\]()]/)) return "bracket";
    if (stream.match(/^[^\s[\]();"]+/)) {
      const w = stream.current().toLowerCase();
      if (state.afterTo) { state.afterTo = false; return "def"; }
      if (w === "to" || w === "to-report") { state.afterTo = true; return "keyword"; }
      if (DECLARATIONS.has(w) || /-own$/.test(w)) return "keyword";
      if (KEYWORDS.has(w)) return "keyword";
      if (/^[-+*/^<>=!]+$/.test(w)) return "operator";
      return "variableName";
    }
    stream.next();
    return null;
  },
  languageData: { commentTokens: { line: ";" }, closeBrackets: { brackets: ["[", "(", '"'] } },
});

const highlight = HighlightStyle.define([
  { tag: t.keyword, color: "var(--syn-keyword)", fontWeight: "600" },
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
  ".cm-activeLine": { backgroundColor: "var(--bg-hover)" },
  ".cm-activeLineGutter": { backgroundColor: "var(--bg-hover)" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": { backgroundColor: "var(--selection) !important" },
  ".cm-tooltip": { backgroundColor: "var(--bg-panel)", color: "var(--text)", border: "1px solid var(--control-border)" },
  ".cm-panels": { backgroundColor: "var(--bg-panel)", color: "var(--text)" },
  ".cm-diagnostic-error": { borderLeft: "4px solid var(--error)" },
  ".cm-lintRange-error": { backgroundImage: "none", textDecoration: "underline wavy var(--error)", textUnderlineOffset: "3px" },
});

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

export function CodeTab() {
  const host = useRef<HTMLDivElement>(null);
  const m = model.value;
  useEffect(() => {
    if (!host.current || !m) return;
    let timer = 0;
    let view: EditorView | null = null;
    let cancelled = false;
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
          rectangularSelection(), highlightActiveLine(), highlightSelectionMatches(), lintGutter(),
          netlogo, syntaxHighlighting(highlight), theme, ...lsp,
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
