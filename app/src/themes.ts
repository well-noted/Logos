// Themes (D-32): several built-in themes and custom themes the user makes from them. Every
// color the interface uses is a token; a theme gives every token a value.

export type Tokens = Record<TokenName, string>;

export const TOKEN_NAMES = [
  "bg", "bgSide", "bgPanel", "bgTitle", "bgInput", "bgHover", "bgSelected",
  "text", "textMuted", "border", "controlBorder", "focus",
  "accent", "accentText", "statusBg", "statusText",
  "error", "warning", "ok",
  "widgetBg", "widgetText", "buttonBg", "buttonText", "buttonActive", "buttonActiveText",
  "gridDot", "codeBg", "codeText", "codeGutter",
  "synKeyword", "synComment", "synString", "synNumber", "synProcedure", "synBracket",
  "selection",
] as const;

export type TokenName = (typeof TOKEN_NAMES)[number];

export interface Theme {
  id: string;
  name: string;
  dark: boolean;
  highContrast: boolean;
  tokens: Tokens;
  /** Interface and code font families. */
  font?: string;
  codeFont?: string;
}

/** What the user can name a token in the theme editor. */
export const TOKEN_LABELS: Record<TokenName, string> = {
  bg: "Window background", bgSide: "Side panels", bgPanel: "Documents and panels",
  bgTitle: "Title bar", bgInput: "Input fields", bgHover: "Hovered item", bgSelected: "Selected item",
  text: "Text", textMuted: "Secondary text", border: "Separators", controlBorder: "Control borders",
  focus: "Keyboard focus", accent: "Primary buttons", accentText: "Primary button text",
  statusBg: "Status bar", statusText: "Status bar text", error: "Errors", warning: "Warnings",
  ok: "Success", widgetBg: "Interface widgets", widgetText: "Widget text", buttonBg: "Model buttons",
  buttonText: "Model button text", buttonActive: "Running button", buttonActiveText: "Running button text",
  gridDot: "Interface grid", codeBg: "Code background", codeText: "Code text", codeGutter: "Line numbers",
  synKeyword: "Keywords", synComment: "Comments", synString: "Strings", synNumber: "Numbers",
  synProcedure: "Procedure names", synBracket: "Brackets", selection: "Text selection",
};

/** Pairs that must be readable: [foreground, background, minimum ratio]. WCAG 2.2 AA asks
 * 4.5:1 for text and 3:1 for controls and focus indicators (A4). */
export const CONTRAST_PAIRS: [TokenName, TokenName, number][] = [
  ["text", "bg", 4.5], ["text", "bgSide", 4.5], ["text", "bgPanel", 4.5], ["text", "bgTitle", 4.5],
  ["text", "bgInput", 4.5], ["text", "bgHover", 4.5], ["text", "bgSelected", 4.5],
  ["textMuted", "bgPanel", 4.5], ["textMuted", "bgSide", 4.5],
  ["accentText", "accent", 4.5], ["statusText", "statusBg", 4.5],
  ["error", "bgPanel", 4.5], ["warning", "bgPanel", 4.5], ["ok", "bgPanel", 4.5],
  ["widgetText", "widgetBg", 4.5], ["buttonText", "buttonBg", 4.5],
  ["buttonActiveText", "buttonActive", 4.5],
  ["codeText", "codeBg", 4.5], ["codeGutter", "codeBg", 4.5],
  ["synKeyword", "codeBg", 4.5], ["synComment", "codeBg", 4.5], ["synString", "codeBg", 4.5],
  ["synNumber", "codeBg", 4.5], ["synProcedure", "codeBg", 4.5], ["synBracket", "codeBg", 4.5],
  ["controlBorder", "bgPanel", 3], ["controlBorder", "widgetBg", 3],
  ["focus", "bgPanel", 3], ["focus", "bgSide", 3], ["focus", "codeBg", 3], ["focus", "widgetBg", 3],
];

const light: Tokens = {
  bg: "#f7f7f5", bgSide: "#f7f7f5", bgPanel: "#ffffff", bgTitle: "#dcdcd8", bgInput: "#ffffff",
  bgHover: "#ebebe7", bgSelected: "#dde3ec",
  text: "#1c1c1c", textMuted: "#55554f", border: "#d0d0cc", controlBorder: "#86867f", focus: "#1f5fbf",
  accent: "#2b2b2b", accentText: "#ffffff", statusBg: "#2b2b2b", statusText: "#f2f2ef",
  error: "#b3261e", warning: "#8a5a00", ok: "#2e6b30",
  widgetBg: "#ffffff", widgetText: "#1c1c1c", buttonBg: "#e3e3df", buttonText: "#1c1c1c",
  buttonActive: "#2b2b2b", buttonActiveText: "#ffffff",
  gridDot: "#cfcfca", codeBg: "#ffffff", codeText: "#1c1c1c", codeGutter: "#6b6b65",
  synKeyword: "#7a2e8f", synComment: "#5f6b5f", synString: "#a3381f", synNumber: "#1f5c99",
  synProcedure: "#0b5f63", synBracket: "#4a4a45", selection: "#cfdcf2",
};

const dark: Tokens = {
  bg: "#1e1f22", bgSide: "#232427", bgPanel: "#28292d", bgTitle: "#18191b", bgInput: "#1e1f22",
  bgHover: "#33353a", bgSelected: "#2f3b50",
  text: "#e6e6e3", textMuted: "#a9a9a3", border: "#3a3c41", controlBorder: "#7b7d83", focus: "#7fb0ff",
  accent: "#e6e6e3", accentText: "#1e1f22", statusBg: "#111214", statusText: "#d8d8d4",
  error: "#ff8a80", warning: "#f0c060", ok: "#8fd694",
  widgetBg: "#2f3034", widgetText: "#e6e6e3", buttonBg: "#3b3d42", buttonText: "#f0f0ec",
  buttonActive: "#e6e6e3", buttonActiveText: "#1e1f22",
  gridDot: "#3c3e43", codeBg: "#1e1f22", codeText: "#e3e3df", codeGutter: "#8d8d88",
  synKeyword: "#d7a6ff", synComment: "#98a898", synString: "#f2a487", synNumber: "#8cc4ff",
  synProcedure: "#6fd3cf", synBracket: "#b9b9b4", selection: "#3a4a66",
};

const hcLight: Tokens = {
  bg: "#ffffff", bgSide: "#ffffff", bgPanel: "#ffffff", bgTitle: "#ffffff", bgInput: "#ffffff",
  bgHover: "#e8e8e8", bgSelected: "#d6e4ff",
  text: "#000000", textMuted: "#2a2a2a", border: "#000000", controlBorder: "#000000", focus: "#0030c0",
  accent: "#000000", accentText: "#ffffff", statusBg: "#000000", statusText: "#ffffff",
  error: "#a00000", warning: "#6a4000", ok: "#005a00",
  widgetBg: "#ffffff", widgetText: "#000000", buttonBg: "#ffffff", buttonText: "#000000",
  buttonActive: "#000000", buttonActiveText: "#ffffff",
  gridDot: "#9a9a9a", codeBg: "#ffffff", codeText: "#000000", codeGutter: "#2a2a2a",
  synKeyword: "#5a0080", synComment: "#2f4a2f", synString: "#8a1a00", synNumber: "#00408a",
  synProcedure: "#004a4d", synBracket: "#000000", selection: "#b8d0ff",
};

const hcDark: Tokens = {
  bg: "#000000", bgSide: "#000000", bgPanel: "#000000", bgTitle: "#000000", bgInput: "#000000",
  bgHover: "#262626", bgSelected: "#16305c",
  text: "#ffffff", textMuted: "#e0e0e0", border: "#ffffff", controlBorder: "#ffffff", focus: "#ffd400",
  accent: "#ffffff", accentText: "#000000", statusBg: "#000000", statusText: "#ffffff",
  error: "#ff9e9e", warning: "#ffd966", ok: "#9eff9e",
  widgetBg: "#000000", widgetText: "#ffffff", buttonBg: "#000000", buttonText: "#ffffff",
  buttonActive: "#ffffff", buttonActiveText: "#000000",
  gridDot: "#5a5a5a", codeBg: "#000000", codeText: "#ffffff", codeGutter: "#e0e0e0",
  synKeyword: "#e6b3ff", synComment: "#b8e0b8", synString: "#ffc2a8", synNumber: "#a8d4ff",
  synProcedure: "#8ff0ec", synBracket: "#ffffff", selection: "#1f4a99",
};

const classic: Tokens = {
  ...light,
  bg: "#e8ecf2", bgSide: "#e8ecf2", bgPanel: "#ffffff", bgTitle: "#c9d2df", bgHover: "#dbe2ec",
  bgSelected: "#c7d6ec", accent: "#2d4f7c", accentText: "#ffffff", statusBg: "#2d4f7c",
  statusText: "#ffffff", buttonBg: "#c9d8ee", buttonText: "#10243f", buttonActive: "#10243f",
  buttonActiveText: "#ffffff", widgetBg: "#f4f6fa", gridDot: "#c4ccd8", focus: "#1f5fbf",
  controlBorder: "#6f7c8f",
};

export const BUILT_IN: Theme[] = [
  { id: "light", name: "Light", dark: false, highContrast: false, tokens: light },
  { id: "dark", name: "Dark", dark: true, highContrast: false, tokens: dark },
  { id: "classic", name: "Classic blue", dark: false, highContrast: false, tokens: classic },
  { id: "hc-light", name: "High contrast light", dark: false, highContrast: true, tokens: hcLight },
  { id: "hc-dark", name: "High contrast dark", dark: true, highContrast: true, tokens: hcDark },
];

// ---- contrast (WCAG 2.2) ------------------------------------------------------------------

export function parseColor(c: string): [number, number, number] | null {
  const m = /^#([0-9a-f]{6})$/i.exec(c.trim());
  if (!m) return null;
  const n = parseInt(m[1], 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function channel(v: number): number {
  const s = v / 255;
  return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
}

export function luminance(c: string): number {
  const rgb = parseColor(c);
  if (!rgb) return 0;
  return 0.2126 * channel(rgb[0]) + 0.7152 * channel(rgb[1]) + 0.0722 * channel(rgb[2]);
}

export function contrast(a: string, b: string): number {
  const la = luminance(a), lb = luminance(b);
  const [hi, lo] = la > lb ? [la, lb] : [lb, la];
  return (hi + 0.05) / (lo + 0.05);
}

export interface ContrastProblem {
  fg: TokenName;
  bg: TokenName;
  ratio: number;
  needed: number;
}

/** The token pairs of a theme that fall below WCAG 2.2 AA. */
export function contrastProblems(tokens: Tokens): ContrastProblem[] {
  const out: ContrastProblem[] = [];
  for (const [fg, bg, needed] of CONTRAST_PAIRS) {
    const ratio = contrast(tokens[fg], tokens[bg]);
    if (ratio < needed) out.push({ fg, bg, ratio: Math.round(ratio * 100) / 100, needed });
  }
  return out;
}

// ---- custom themes ------------------------------------------------------------------------

/** A custom theme as stored: a base built-in theme and the tokens the user changed. */
export interface CustomThemeFile {
  format: 1;
  name: string;
  base: string;
  tokens: Partial<Tokens>;
  font?: string;
  codeFont?: string;
}

export function customTheme(file: CustomThemeFile): Theme {
  const base = BUILT_IN.find((t) => t.id === file.base) ?? BUILT_IN[0];
  const tokens = { ...base.tokens };
  for (const name of TOKEN_NAMES) {
    const v = file.tokens[name];
    if (typeof v === "string" && parseColor(v)) tokens[name] = v;
  }
  return {
    id: `custom:${file.name}`, name: file.name, dark: base.dark, highContrast: base.highContrast,
    tokens, font: file.font, codeFont: file.codeFont,
  };
}

export function parseCustomTheme(json: string): CustomThemeFile | null {
  try {
    const v = JSON.parse(json);
    if (typeof v !== "object" || v === null || typeof v.name !== "string") return null;
    return {
      format: 1, name: v.name, base: typeof v.base === "string" ? v.base : "light",
      tokens: typeof v.tokens === "object" && v.tokens ? v.tokens : {},
      font: typeof v.font === "string" ? v.font : undefined,
      codeFont: typeof v.codeFont === "string" ? v.codeFont : undefined,
    };
  } catch {
    return null;
  }
}

function cssName(t: TokenName): string {
  return "--" + t.replace(/[A-Z]/g, (c) => "-" + c.toLowerCase());
}

/** Sets a theme's tokens as CSS variables on the document. */
export function applyTheme(theme: Theme, fontSize: number, codeFontSize: number) {
  const root = document.documentElement;
  for (const name of TOKEN_NAMES) root.style.setProperty(cssName(name), theme.tokens[name]);
  root.style.setProperty("--font", theme.font || '"IBM Plex Sans", system-ui, sans-serif');
  root.style.setProperty("--code-font", theme.codeFont || '"IBM Plex Mono", ui-monospace, monospace');
  root.style.setProperty("--font-size", `${fontSize || 13}px`);
  root.style.setProperty("--code-font-size", `${codeFontSize || 13}px`);
  root.dataset.theme = theme.id;
  root.style.colorScheme = theme.dark ? "dark" : "light";
}

/** The theme for a setting: an id, `custom:<name>`, or empty to follow the system. */
export function resolveTheme(setting: string, customs: Theme[]): Theme {
  if (setting) {
    const found = BUILT_IN.find((t) => t.id === setting) ?? customs.find((t) => t.id === setting);
    if (found) return found;
  }
  const media = (q: string) => typeof window !== "undefined" && window.matchMedia?.(q).matches;
  const dark = media("(prefers-color-scheme: dark)");
  const more = media("(prefers-contrast: more)") || media("(forced-colors: active)");
  if (more) return BUILT_IN.find((t) => t.id === (dark ? "hc-dark" : "hc-light"))!;
  return BUILT_IN.find((t) => t.id === (dark ? "dark" : "light"))!;
}
