// NetLogo names by class, for highlighting in the Code tab and the Command Center. The
// classes come from NetLogo's primitive table through the host (D-01) and from the model's
// own declarations (the outline and the interface's variables). The language server's parser
// (D-11) stays the real one; this is coloring only.

import { signal } from "@preact/signals";
import { invoke } from "@tauri-apps/api/core";
import { StreamLanguage } from "@codemirror/language";
import { Tag, tags as t } from "@lezer/highlight";

export interface LanguageNames {
  commands: string[]; reporters: string[]; keywords: string[]; constants: string[]; variables: string[];
  breedPatterns: [string, string, string][];
}

export const languageNames = signal<LanguageNames | null>(null);
let loading = false;

export function loadLanguageNames() {
  if (languageNames.value || loading) return;
  loading = true;
  invoke<LanguageNames>("language_names")
    .then((n) => { languageNames.value = n; })
    .catch(() => { /* the engine isn't ready; tried again on the next model */ })
    .finally(() => { loading = false; });
}

/** Names the model itself defines. */
export interface ModelNames {
  commands: string[]; reporters: string[]; variables: string[];
  /** (plural, singular, kind of breed declaration). */
  breeds: [string, string, string][];
}

export type NameClass = "keyword" | "command" | "reporter" | "constant" | "variable" | "procedure" | "breed";

/** Highlighting classes for the colors in each theme. */
export const tagCommand = Tag.define();
export const tagReporter = Tag.define();
export const tagUserProcedure = Tag.define();
export const tagVariable = Tag.define();

const FALLBACK_KEYWORDS = [
  "to", "to-report", "end", "globals", "breed", "directed-link-breed", "undirected-link-breed",
  "extensions", "__includes", "turtles-own", "patches-own", "links-own",
];

export class Classifier {
  private map = new Map<string, NameClass>();

  constructor(lang: LanguageNames | null, model: ModelNames | null) {
    const put = (names: Iterable<string>, c: NameClass) => { for (const n of names) this.map.set(n.toLowerCase(), c); };
    put(FALLBACK_KEYWORDS, "keyword");
    if (lang) {
      put(lang.variables, "variable");
      put(lang.reporters, "reporter");
      put(lang.commands, "command");
      put(lang.constants, "constant");
      put(lang.keywords, "keyword");
    }
    if (model) {
      put(model.variables, "variable");
      put(model.reporters, "procedure");
      put(model.commands, "procedure");
      for (const [plural, singular, kind] of model.breeds) {
        this.map.set(plural.toLowerCase(), "breed");
        if (singular) this.map.set(singular.toLowerCase(), "breed");
        for (const [pattern, k, bk] of lang?.breedPatterns ?? []) {
          if (bk !== kind) continue;
          const name = pattern.replace("<breeds>", plural).replace("<breed>", singular).toLowerCase();
          this.map.set(name, k === "command" ? "command" : "reporter");
        }
      }
    }
  }

  classify(word: string): NameClass | null {
    const w = word.toLowerCase();
    if (w.endsWith("-own")) return "keyword";
    return this.map.get(w) ?? null;
  }
}

/** Names the model declares, from the outline entries and the interface's variables. */
export function modelNames(outline: { name: string; kind: string; detail?: string | null }[], interfaceGlobals: string[]): ModelNames {
  const m: ModelNames = { commands: [], reporters: [], variables: [...interfaceGlobals], breeds: [] };
  for (const e of outline) {
    if (e.kind === "procedure") m.commands.push(e.name);
    else if (e.kind === "reporter") m.reporters.push(e.name);
    else if ((e.kind === "globals" || e.kind === "variables") && e.detail) m.variables.push(...e.detail.split(/\s+/).filter(Boolean));
    else if (e.kind === "breed" && e.detail) {
      const [plural, singular] = e.detail.split(/\s+/);
      const kind = e.name.toLowerCase().startsWith("directed") ? "directed-link"
        : e.name.toLowerCase().startsWith("undirected") ? "undirected-link" : "turtle";
      m.breeds.push([plural, singular ?? "", kind]);
    }
  }
  return m;
}

interface NlState { afterTo: boolean }

const NUMBER = /^-?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?(?![\w\-?!.])/;

/** A tokenizer for one line of NetLogo; used by the editor language and by the Command
 * Center's echo. */
export function tokenize(line: string, c: Classifier): { text: string; cls: string | null }[] {
  const out: { text: string; cls: string | null }[] = [];
  let i = 0;
  let afterTo = false;
  while (i < line.length) {
    const rest = line.slice(i);
    let m: RegExpMatchArray | null;
    if ((m = rest.match(/^\s+/))) { out.push({ text: m[0], cls: null }); i += m[0].length; continue; }
    if (rest[0] === ";") { out.push({ text: rest, cls: "comment" }); break; }
    if ((m = rest.match(/^"(?:[^"\\]|\\.)*"?/))) { out.push({ text: m[0], cls: "string" }); i += m[0].length; continue; }
    if ((m = rest.match(NUMBER))) { out.push({ text: m[0], cls: "number" }); i += m[0].length; continue; }
    if ((m = rest.match(/^[[\]()]/))) { out.push({ text: m[0], cls: "bracket" }); i += 1; continue; }
    if ((m = rest.match(/^[^\s[\]();"]+/))) {
      const w = m[0];
      let cls: string | null;
      if (afterTo) { cls = "definition"; afterTo = false; }
      else {
        const k = c.classify(w);
        if (w.toLowerCase() === "to" || w.toLowerCase() === "to-report") afterTo = true;
        cls = k === "constant" ? "number" : k;
      }
      out.push({ text: w, cls });
      i += w.length;
      continue;
    }
    out.push({ text: rest[0], cls: null });
    i += 1;
  }
  return out;
}

/** The CodeMirror language for a classifier. */
export function netlogoLanguage(c: Classifier) {
  return StreamLanguage.define<NlState>({
    name: "netlogo",
    startState: () => ({ afterTo: false }),
    token(stream, state) {
      if (stream.eatSpace()) return null;
      if (stream.match(";")) { stream.skipToEnd(); return "comment"; }
      if (stream.match(/^"(?:[^"\\]|\\.)*"?/)) return "string";
      if (stream.match(NUMBER)) return "number";
      if (stream.match(/^[[\]()]/)) return "bracket";
      if (stream.match(/^[^\s[\]();"]+/)) {
        const w = stream.current().toLowerCase();
        if (state.afterTo) { state.afterTo = false; return "def"; }
        if (w === "to" || w === "to-report") { state.afterTo = true; return "keyword"; }
        const k = c.classify(w);
        if (k === "keyword") return "keyword";
        if (k === "constant") return "number";
        if (k === "command") return "nlCommand";
        if (k === "reporter") return "nlReporter";
        if (k === "procedure") return "nlProcedure";
        if (k === "variable" || k === "breed") return "nlVariable";
        if (/^[-+*/^<>=!]+$/.test(w)) return "operator";
        return "variableName";
      }
      stream.next();
      return null;
    },
    tokenTable: { nlCommand: tagCommand, nlReporter: tagReporter, nlProcedure: tagUserProcedure, nlVariable: tagVariable },
    // NetLogo names include these characters, so a double click selects `num-occupying-households`
    // whole and matches are found for the whole name.
    languageData: { commentTokens: { line: ";" }, closeBrackets: { brackets: ["[", "(", '"'] }, wordChars: "-_?!.=*<>/+:#%$^'&" },
  });
}

export { t as tags };
