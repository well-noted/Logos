// Settings: themes (D-32), the custom theme editor, and font sizes.

import { useEffect, useRef, useState } from "preact/hooks";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "./icons";
import { errorText, ipc } from "./ipc";
import {
  BUILT_IN, TOKEN_LABELS, TOKEN_NAMES, contrastProblems, customTheme, parseCustomTheme, applyTheme,
  type CustomThemeFile, type Theme, type TokenName,
} from "./themes";
import { customThemes, loadCustomThemes, refreshTheme, saveSettings, settings, settingsOpen, showToast, theme } from "./state";

function Swatch({ t }: { t: Theme }) {
  const k = t.tokens;
  return (
    <span class="sw" aria-hidden="true">
      <span style={{ background: k.bgPanel }} /><span style={{ background: k.bgSide }} />
      <span style={{ background: k.accent }} /><span style={{ background: k.synKeyword }} /><span style={{ background: k.text }} />
    </span>
  );
}

export function SettingsDialog() {
  const s = settings.value;
  const [editing, setEditing] = useState<CustomThemeFile | null>(null);
  const close = () => { settingsOpen.value = false; refreshTheme(); };
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => { ref.current?.querySelector<HTMLElement>("button, input, select")?.focus(); }, []);
  const all: { id: string; name: string; theme?: Theme }[] = [
    { id: "", name: "Follow the system" },
    ...BUILT_IN.map((t) => ({ id: t.id, name: t.name, theme: t })),
    ...customThemes.value.map((t) => ({ id: t.id, name: t.name, theme: t })),
  ];
  return (
    <div class="overlay" onClick={close} onKeyDown={(e) => { if (e.key === "Escape") close(); }}>
      <div class="settings" role="dialog" aria-modal="true" aria-labelledby="settings-h" ref={ref} onClick={(e) => e.stopPropagation()}>
        <header>
          <h2 id="settings-h">Settings</h2>
          <button class="icon-btn" aria-label="Close settings" onClick={close}><Icon name="close" /></button>
        </header>
        <div class="body">
          <section aria-labelledby="themes-h">
            <h3 id="themes-h" style="margin: 0 0 8px">Theme</h3>
            <div class="theme-grid" role="radiogroup" aria-labelledby="themes-h">
              {all.map((t) => (
                <button class="theme-card" role="radio" aria-checked={s.theme === t.id}
                  onClick={() => saveSettings({ theme: t.id })}>
                  {t.theme ? <Swatch t={t.theme} /> : <span class="sw" aria-hidden="true"><span style="background:#fff" /><span style="background:#1e1f22" /></span>}
                  <div>{t.name}</div>
                </button>
              ))}
            </div>
            <div class="actions" style="margin-top: 10px">
              <button class="btn" onClick={() => setEditing({ format: 1, name: "My theme", base: theme.value.id.startsWith("custom:") ? "light" : theme.value.id, tokens: {} })}>
                <Icon name="add" /> New custom theme…
              </button>
              {s.theme.startsWith("custom:") && (
                <button class="btn" onClick={() => {
                  const t = customThemes.value.find((c) => c.id === s.theme);
                  if (t) setEditing({ format: 1, name: t.name, base: baseOf(t), tokens: { ...t.tokens }, font: t.font, codeFont: t.codeFont });
                }}><Icon name="edit" /> Edit this theme…</button>
              )}
              <button class="btn" onClick={importTheme}>Import…</button>
            </div>
          </section>
          {editing && <ThemeEditor file={editing} onDone={() => setEditing(null)} />}
          <section aria-labelledby="family-h">
            <h3 id="family-h" style="margin: 0 0 8px">Fonts</h3>
            <div class="font-pickers">
              <FontPicker label="Interface font" value={(s.fontFamily as string) || ""}
                onChange={(f) => saveSettings({ fontFamily: f || null })} />
              <FontPicker label="Code font" monoFirst value={(s.codeFontFamily as string) || ""}
                onChange={(f) => saveSettings({ codeFontFamily: f || null })} />
            </div>
          </section>
          <section aria-labelledby="confirm-h">
            <h3 id="confirm-h" style="margin: 0 0 8px">Confirmations</h3>
            <label><input type="checkbox" checked={s.confirmApply !== false}
              onChange={(e) => saveSettings({ confirmApply: (e.target as HTMLInputElement).checked })} /> Ask before Apply saves and reloads the model</label>
          </section>
          <section aria-labelledby="fonts-h">
            <h3 id="fonts-h" style="margin: 0 0 8px">Text size</h3>
            <div class="actions">
              <label>Interface <select value={String(s.fontSize || 13)} onChange={(e) => saveSettings({ fontSize: Number((e.target as HTMLSelectElement).value) })}>
                {[12, 13, 14, 15, 16, 18, 20].map((n) => <option value={String(n)}>{n} px</option>)}
              </select></label>
              <label>Code <select value={String(s.codeFontSize || 13)} onChange={(e) => saveSettings({ codeFontSize: Number((e.target as HTMLSelectElement).value) })}>
                {[12, 13, 14, 15, 16, 18, 20, 24].map((n) => <option value={String(n)}>{n} px</option>)}
              </select></label>
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}

function baseOf(t: Theme): string {
  return t.dark ? (t.highContrast ? "hc-dark" : "dark") : t.highContrast ? "hc-light" : "light";
}

async function importTheme() {
  const path = await openDialog({ multiple: false, filters: [{ name: "Logos theme", extensions: ["json"] }] });
  if (typeof path !== "string") return;
  try {
    const json = await invoke<string>("theme_import", { path });
    const file = parseCustomTheme(json);
    if (!file) throw new Error("This file isn't a Logos theme.");
    await ipc.themeSave(file.name, JSON.stringify(file, null, 2));
    await loadCustomThemes();
    await saveSettings({ theme: `custom:${file.name}` });
    showToast(`Imported theme ${file.name}`);
  } catch (e) {
    showToast(`Can't import: ${errorText(e)}`);
  }
}

function ThemeEditor({ file, onDone }: { file: CustomThemeFile; onDone: () => void }) {
  const [draft, setDraft] = useState<CustomThemeFile>(file);
  const preview = customTheme(draft);
  const problems = contrastProblems(preview.tokens);
  // Live preview: the whole window shows the draft while the editor is open.
  useEffect(() => { applyTheme(preview, settings.value.fontSize, settings.value.codeFontSize,
    settings.value.fontFamily as string | undefined, settings.value.codeFontFamily as string | undefined); }, [JSON.stringify(draft)]);
  useEffect(() => () => refreshTheme(), []);
  const set = (name: TokenName, value: string) => setDraft({ ...draft, tokens: { ...draft.tokens, [name]: value } });
  const save = async () => {
    const json = JSON.stringify({ ...draft, tokens: preview.tokens }, null, 2);
    try {
      await ipc.themeSave(draft.name, json);
      await loadCustomThemes();
      await saveSettings({ theme: `custom:${draft.name}` });
      showToast(`Saved theme ${draft.name}`);
      onDone();
    } catch (e) {
      showToast(`Can't save the theme: ${errorText(e)}`);
    }
  };
  const exportFile = async () => {
    const path = await saveDialog({ defaultPath: `${draft.name}.json`, filters: [{ name: "Logos theme", extensions: ["json"] }] });
    if (!path) return;
    try {
      await invoke("theme_export", { path, json: JSON.stringify({ ...draft, tokens: preview.tokens }, null, 2) });
      showToast("Theme exported");
    } catch (e) { showToast(`Can't export: ${errorText(e)}`); }
  };
  const remove = async () => {
    try {
      await ipc.themeDelete(draft.name);
      await loadCustomThemes();
      if (settings.value.theme === `custom:${draft.name}`) await saveSettings({ theme: "" });
      onDone();
    } catch (e) { showToast(errorText(e)); }
  };
  return (
    <section aria-labelledby="editor-h" class="card">
      <h3 id="editor-h" style="margin: 0 0 8px">Custom theme</h3>
      <div class="actions" style="margin-bottom: 10px">
        <label>Name <input type="text" value={draft.name} onInput={(e) => setDraft({ ...draft, name: (e.target as HTMLInputElement).value })} /></label>
        <label>Start from <select value={draft.base} onChange={(e) => setDraft({ ...draft, base: (e.target as HTMLSelectElement).value, tokens: {} })}>
          {BUILT_IN.map((t) => <option value={t.id}>{t.name}</option>)}
        </select></label>
        <label>Interface font <input type="text" placeholder="IBM Plex Sans" value={draft.font ?? ""} onInput={(e) => setDraft({ ...draft, font: (e.target as HTMLInputElement).value || undefined })} /></label>
        <label>Code font <input type="text" placeholder="IBM Plex Mono" value={draft.codeFont ?? ""} onInput={(e) => setDraft({ ...draft, codeFont: (e.target as HTMLInputElement).value || undefined })} /></label>
      </div>
      <div class="token-grid">
        {TOKEN_NAMES.map((n) => (
          <div class="token-row">
            <input type="color" aria-label={TOKEN_LABELS[n]} value={preview.tokens[n]} onInput={(e) => set(n, (e.target as HTMLInputElement).value)} />
            <input class="hex" type="text" aria-label={`${TOKEN_LABELS[n]} hex value`} value={preview.tokens[n]}
              onChange={(e) => set(n, (e.target as HTMLInputElement).value)} />
            <span>{TOKEN_LABELS[n]}</span>
          </div>
        ))}
      </div>
      <div role="status" style="margin-top: 10px">
        {problems.length === 0
          ? <p class="ok" style="margin: 0">Every text and control color meets WCAG 2.2 AA contrast.</p>
          : (
            <div>
              <p style="margin: 0 0 4px; color: var(--warning)">Below WCAG 2.2 AA contrast. You can still save, but some text or controls may be hard to read:</p>
              <ul class="problems-list">
                {problems.map((p) => <li>{TOKEN_LABELS[p.fg]} on {TOKEN_LABELS[p.bg]}: {p.ratio}:1, needs {p.needed}:1</li>)}
              </ul>
            </div>
          )}
      </div>
      <div class="actions" style="margin-top: 10px">
        <button class="btn primary" onClick={save}>Save theme</button>
        <button class="btn" onClick={exportFile}>Export…</button>
        {customThemes.value.some((t) => t.name === draft.name) && <button class="btn" onClick={remove}>Delete</button>}
        <button class="btn" onClick={onDone}>Cancel</button>
      </div>
    </section>
  );
}

interface FontFamily { family: string; monospace: boolean }
let fontCache: FontFamily[] | null = null;

/** A searchable list of the fonts installed on the computer, each shown in its own font. */
function FontPicker({ label, value, onChange, monoFirst }: { label: string; value: string; onChange: (f: string) => void; monoFirst?: boolean }) {
  const [fonts, setFonts] = useState<FontFamily[] | null>(fontCache);
  const [filter, setFilter] = useState("");
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (fontCache) return;
    invoke<FontFamily[]>("system_fonts").then((f) => { fontCache = f; setFonts(f); }).catch(() => setFonts([]));
  }, []);
  const list = (fonts ?? [])
    .filter((f) => !filter || f.family.toLowerCase().includes(filter.toLowerCase()))
    .sort((a, b) => (monoFirst ? Number(b.monospace) - Number(a.monospace) : 0) || a.family.localeCompare(b.family));
  const id = label.replace(/\W+/g, "-").toLowerCase();
  return (
    <div class="font-picker">
      <label for={`${id}-btn`}>{label}</label>
      <button id={`${id}-btn`} class="btn font-current" aria-haspopup="listbox" aria-expanded={open}
        style={value ? { fontFamily: `"${value}"` } : undefined} onClick={() => setOpen(!open)}>
        {value || "Theme default (IBM Plex)"}
      </button>
      {open && (
        <div class="font-pop">
          <input type="text" placeholder="Filter fonts" aria-label={`Filter ${label.toLowerCase()} list`} value={filter} autoFocus
            onInput={(e) => setFilter((e.target as HTMLInputElement).value)}
            onKeyDown={(e) => { if (e.key === "Escape") setOpen(false); }} />
          <ul role="listbox" aria-label={label}>
            <li role="option" aria-selected={!value}><button onClick={() => { onChange(""); setOpen(false); }}>Theme default (IBM Plex)</button></li>
            {fonts === null && <li class="muted">Reading the installed fonts…</li>}
            {list.slice(0, 400).map((f) => (
              <li role="option" aria-selected={f.family === value}>
                <button style={{ fontFamily: `"${f.family}"` }} onClick={() => { onChange(f.family); setOpen(false); }}>
                  {f.family} <span class="sample">to go ask turtles [ fd 1 ] 0123</span>
                  {f.monospace && <span class="tag">monospace</span>}
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
