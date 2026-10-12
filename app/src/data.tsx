// A project data file (spec part P): a table preview, with the data dictionary beside it and
// a form to edit the dictionary. The dictionary file sits next to the data file and is named
// in logos.toml.

import { useEffect, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "./icons";
import { errorText } from "./ipc";
import { addActivity, model, refreshProject, showToast } from "./state";

interface ColumnView { name: string; type: string; unit: string; description: string }
interface DictionaryView { description: string; source: string; format: string; columns: ColumnView[] }
interface DataPreview {
  path: string; role: string; exists: boolean; bytes: number; layout: "lines" | "columns" | "text";
  rows: string[][]; more: boolean; values: number; text: string;
  dictionaryPath: string; dictionary: DictionaryView | null; dictionaryError: string | null;
}

const TYPES = ["", "number", "integer", "string", "boolean", "date", "other"];

function size(b: number): string {
  return b < 1024 ? `${b} bytes` : b < 1 << 20 ? `${(b / 1024).toFixed(1)} KB` : `${(b / (1 << 20)).toFixed(1)} MB`;
}

export function DataDoc({ path }: { path: string }) {
  const m = model.value!;
  const [p, setP] = useState<DataPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [draft, setDraft] = useState<DictionaryView | null>(null);
  const [view, setView] = useState<"table" | "text">("table");
  const load = () => invoke<DataPreview>("data_preview", { id: m.id, path })
    .then((r) => { setP(r); setDraft(r.dictionary); setView(r.layout === "text" ? "text" : "table"); setError(null); })
    .catch((e) => setError(errorText(e)));
  useEffect(() => { void load(); }, [m.id, path]);
  if (error) return <div class="doc-page"><p class="err">{error}</p></div>;
  if (!p) return <div class="doc-page"><p class="muted">Reading {path}…</p></div>;

  const d = draft;
  const changed = JSON.stringify(d) !== JSON.stringify(p.dictionary);
  const save = async () => {
    if (!d) return;
    try {
      const r = await invoke<DataPreview>("dictionary_save", { id: m.id, path, dictionary: d });
      setP(r); setDraft(r.dictionary);
      addActivity(`Saved the data dictionary ${r.dictionaryPath}`);
      showToast(`Saved ${r.dictionaryPath}`);
      refreshProject();
    } catch (e) { showToast(errorText(e)); }
  };
  const setCol = (i: number, patch: Partial<ColumnView>) => {
    if (!d) return;
    setDraft({ ...d, columns: d.columns.map((c, j) => (j === i ? { ...c, ...patch } : c)) });
  };
  const width = Math.max(d?.columns.length ?? 0, ...p.rows.map((r) => r.length));
  const header = Array.from({ length: width }, (_, i) => d?.columns[i]?.name || String(i + 1));

  return (
    <div class="doc-page data-doc">
      <h2 class="mono" style="font-size: 16px">{p.path}</h2>
      <p class="muted">
        {p.role} file · {p.exists ? size(p.bytes) : "doesn't exist yet"}
        {p.layout !== "text" && p.exists ? ` · ${p.values} values` : ""}
        {p.layout === "columns" ? `, grouped into rows of ${d?.columns.length} by the dictionary's columns` : ""}
      </p>
      <div class="data-split">
        <section aria-label="Preview">
          <div class="subtabs" role="tablist" aria-label="Preview as">
            <button role="tab" aria-selected={view === "table"} disabled={p.layout === "text"} onClick={() => setView("table")}>Table</button>
            <button role="tab" aria-selected={view === "text"} onClick={() => setView("text")}>Text</button>
            <span class="spacer" />
            <button class="btn small" onClick={() => void load()}>Read again</button>
          </div>
          {!p.exists ? <p class="empty-note">The model writes this file when it runs.</p>
            : view === "text" ? <pre class="log" style="max-height: 60vh">{p.text}{p.text.length >= 8000 ? "\n…" : ""}</pre>
            : (
              <div class="table-scroll">
                {p.layout === "lines" && p.rows.length === 1 && p.values > 20 && !d?.columns.length && (
                  <p class="empty-note" style="padding: 4px 0">The file is one line of {p.values} values. Describe its columns in the dictionary to see it as rows.</p>
                )}
                <table class="grid-table mono">
                  <thead><tr><th>#</th>{header.map((h) => <th>{h}</th>)}</tr></thead>
                  <tbody>
                    {p.rows.map((r, i) => (
                      <tr><td class="muted">{i + 1}</td>{header.map((_, j) => <td>{r[j] ?? ""}</td>)}</tr>
                    ))}
                  </tbody>
                </table>
                {p.more && <p class="muted" style="font-size: 12px">The first {p.rows.length} rows are shown.</p>}
              </div>
            )}
        </section>
        <section aria-label="Data dictionary" class="dictionary">
          <h3 style="margin-top: 0">Data dictionary</h3>
          <p class="muted" style="font-size: 12px; margin-top: 0"><span class="mono">{p.dictionaryPath}</span>{p.dictionary ? "" : " (not written yet)"}</p>
          {p.dictionaryError && <p class="err" role="alert">The dictionary can't be read: {p.dictionaryError}. Saving here replaces what it can't read.</p>}
          {!d ? (
            <button class="btn small" onClick={() => setDraft({
              description: "", source: "", format: p.layout === "columns" || p.layout === "lines" ? "whitespace-separated, no header" : "",
              columns: p.layout === "lines" && p.rows[0] && p.rows[0].length <= 40 ? p.rows[0].map((_, i) => ({ name: `column-${i + 1}`, type: "", unit: "", description: "" })) : [],
            })}>Describe this file…</button>
          ) : (
            <>
              <div class="form-grid" style="grid-template-columns: 90px 1fr">
                <label for="dd-desc">Description</label>
                <input id="dd-desc" type="text" value={d.description} onInput={(e) => setDraft({ ...d, description: (e.target as HTMLInputElement).value })} />
                <label for="dd-source">Source</label>
                <input id="dd-source" type="text" value={d.source} onInput={(e) => setDraft({ ...d, source: (e.target as HTMLInputElement).value })} />
                <label for="dd-format">Format</label>
                <input id="dd-format" type="text" value={d.format} onInput={(e) => setDraft({ ...d, format: (e.target as HTMLInputElement).value })} />
              </div>
              <table class="grid-table" style="margin-top: 8px">
                <thead><tr><th>#</th><th>Column</th><th>Type</th><th>Unit</th><th>Description</th><th /></tr></thead>
                <tbody>
                  {d.columns.map((c, i) => (
                    <tr>
                      <td class="muted">{i + 1}</td>
                      <td><input type="text" class="mono" aria-label={`Name of column ${i + 1}`} value={c.name} onInput={(e) => setCol(i, { name: (e.target as HTMLInputElement).value })} /></td>
                      <td><select aria-label={`Type of column ${i + 1}`} value={c.type} onChange={(e) => setCol(i, { type: (e.target as HTMLSelectElement).value })}>
                        {TYPES.map((t) => <option value={t}>{t || "–"}</option>)}
                      </select></td>
                      <td><input type="text" aria-label={`Unit of column ${i + 1}`} value={c.unit} style="width: 70px" onInput={(e) => setCol(i, { unit: (e.target as HTMLInputElement).value })} /></td>
                      <td><input type="text" aria-label={`Description of column ${i + 1}`} value={c.description} onInput={(e) => setCol(i, { description: (e.target as HTMLInputElement).value })} /></td>
                      <td><button class="icon-btn" aria-label={`Remove column ${i + 1}`} onClick={() => setDraft({ ...d, columns: d.columns.filter((_, j) => j !== i) })}><Icon name="close" /></button></td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <div class="actions">
                <button class="btn small" onClick={() => setDraft({ ...d, columns: [...d.columns, { name: "", type: "", unit: "", description: "" }] })}><Icon name="add" /> Column</button>
                <span style="flex: 1" />
                {changed && <button class="btn small" onClick={() => setDraft(p.dictionary)}>Discard</button>}
                <button class="btn small primary" disabled={!changed} onClick={save}>Save dictionary</button>
              </div>
            </>
          )}
        </section>
      </div>
    </div>
  );
}
