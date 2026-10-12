// Projects and secrets: the Files panel, the project setup review (design 3.9) and the
// Secrets document (design 3.7).

import { useEffect, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { ask } from "@tauri-apps/plugin-dialog";
import { Icon } from "./icons";
import { errorText } from "./ipc";
import {
  addActivity, closeDoc, findings, model, openDoc, project, refreshProject, showToast, type ProjectView,
} from "./state";

const ROLES = ["input", "output", "code", "config"];

// ---- the Files panel ----------------------------------------------------------------------

/** Opens a project file: data files (input and output) get a preview with their dictionary. */
function openFile(path: string, role: string) {
  if (role === "input" || role === "output") openDoc({ id: `data-${path}`, kind: "data", path });
  else if (path.toLowerCase().endsWith(".experiments.toml")) showToast("The Logos settings of each experiment are on its page in the Experiments activity.");
  else showToast(`Logos doesn't open ${role} files yet.`);
}

export function FilesPanel() {
  const p = project.value;
  const m = model.value;
  if (!m) return <p class="empty-note">Open a model to see the files around it.</p>;
  if (!p) return <p class="empty-note">Reading the folder…</p>;
  if (!p.isProject) {
    return (
      <div class="empty-note">
        <p style="margin-top:0">This folder isn't a Logos project yet. The model runs without one.</p>
        <p>A project lists the files the model reads and writes, the secrets it needs and the Python it uses, in a <span class="mono">logos.toml</span> file NetLogo ignores.</p>
        <button class="btn primary small" onClick={() => openDoc({ id: "project", kind: "project" })}>Make project…</button>
      </div>
    );
  }
  const errors = findings.value.filter((f) => f.severity === "error").length;
  const warnings = findings.value.filter((f) => f.severity === "warning").length;
  return (
    <div>
      <ul class="tree" aria-label="Project files">
        <li><button title={`${p.dir}`} onClick={() => openDoc({ id: "project", kind: "project" })}>
          <span class="kind">project</span><span>logos.toml</span></button></li>
        <li><button><span class="kind">model</span><span>{p.model}</span></button></li>
        {p.files.map((f) => (
          <li><button title={f.exists ? f.path : `${f.path} (doesn't exist yet)`}
            onClick={() => openFile(f.path, f.role)}>
            <span class="kind">{f.role}</span>
            <span style={f.exists ? undefined : "color: var(--text-muted); font-style: italic"}>{f.path}</span>
          </button></li>
        ))}
      </ul>
      <div class="panel-head" style="padding: 8px 6px 0"><span>Secrets</span></div>
      <p class="empty-note" style="padding-top: 2px">
        {p.required.length + p.optional.length === 0 ? "None declared." : `${p.required.length} required, ${p.optional.length} optional.`}
      </p>
      <div style="padding: 0 6px"><button class="btn small" onClick={() => openDoc({ id: "secrets", kind: "secrets" })}>Secrets…</button></div>
      <div class="panel-head" style="padding: 8px 6px 0"><span>Check</span></div>
      <p class="empty-note" style="padding-top: 2px" role="status">
        {errors ? <span class="err">{errors} error{errors > 1 ? "s" : ""}</span> : <span class="ok">No errors</span>}
        {warnings ? `, ${warnings} warning${warnings > 1 ? "s" : ""}` : ""}. See Problems.
      </p>
      <div style="padding: 0 6px"><button class="btn small" onClick={() => refreshProject()}>Check again</button></div>
    </div>
  );
}

// ---- project setup review (design 3.9) ----------------------------------------------------

interface Detection {
  model: string;
  files: { path: string; role?: string | null; language?: string | null; reasons: string[]; exists: boolean }[];
  dynamic: [string, number][];
  secretNames: string[];
  requirements?: string | null;
  python?: string | null;
  skippedFolders: [string, number][];
}

export function ProjectDoc() {
  const p = project.value;
  const m = model.value;
  if (!m || !p) return null;
  return p.isProject ? <ProjectSummary p={p} /> : <SetupReview />;
}

function ProjectSummary({ p }: { p: ProjectView }) {
  const m = model.value!;
  const setRole = async (path: string, role: string) => {
    try {
      project.value = await invoke<ProjectView>("project_set_role", { id: m.id, path, role });
      addActivity(`Set ${path} to ${role}`);
      refreshProject();
    } catch (e) { showToast(errorText(e)); }
  };
  return (
    <div class="doc-page">
      <h2>Project</h2>
      <p class="muted">Folder <span class="mono">{p.dir}</span> · model <span class="mono">{p.model}</span>{p.engine ? <> · engine <span class="mono">{p.engine}</span></> : null}</p>
      <h3>Files</h3>
      <table class="grid-table">
        <thead><tr><th>File</th><th>Role</th><th>Exists</th></tr></thead>
        <tbody>
          {p.files.map((f) => (
            <tr>
              <td class="mono"><button class="link-btn" onClick={() => openFile(f.path, f.role)}>{f.path}</button></td>
              <td>
                <select aria-label={`Role of ${f.path}`} value={f.role} onChange={(e) => setRole(f.path, (e.target as HTMLSelectElement).value)}>
                  {ROLES.map((r) => <option value={r}>{r}</option>)}
                  <option value="none">remove from project</option>
                </select>
              </td>
              <td>{f.exists ? "yes" : <span class="muted">not yet</span>}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <h3>Check</h3>
      <ul class="findings">
        {findings.value.map((f) => (
          <li class={f.severity === "error" ? "err" : f.severity === "warning" ? "lvl-warning" : "ok"}>
            <strong>{f.severity === "ok" ? "✓" : f.severity === "error" ? "✕" : "!"}</strong> {f.subject}: {f.message}
          </li>
        ))}
      </ul>
      <div class="actions"><button class="btn" onClick={() => refreshProject()}>Check again</button>
        <button class="btn" onClick={() => openDoc({ id: "secrets", kind: "secrets" })}>Secrets…</button></div>
      <h3>logos.toml</h3>
      <pre class="log" style="max-height: 320px">{p.source}</pre>
    </div>
  );
}

function SetupReview() {
  const m = model.value!;
  const [d, setD] = useState<Detection | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [choice, setChoice] = useState<Record<string, string>>({});
  const [secrets, setSecrets] = useState<Record<string, string>>({});
  useEffect(() => {
    invoke<Detection>("project_detect", { id: m.id, code: m.code }).then((r) => {
      setD(r);
      const c: Record<string, string> = {};
      // The Logos experiment settings file (D-27) is a config file of the project.
      const settingsFile = `${r.model.replace(/\.nlogox$/i, "")}.experiments.toml`;
      for (const f of r.files) c[f.path] = f.path === settingsFile ? "config" : f.role ?? "skip";
      setChoice(c);
      const s: Record<string, string> = {};
      for (const n of r.secretNames) s[n] = "optional";
      setSecrets(s);
    }).catch((e) => setError(errorText(e)));
  }, [m.id]);
  if (error) return <div class="doc-page"><p class="err">{error}</p></div>;
  if (!d) return <div class="doc-page"><p class="muted">Reading the model and its folder…</p></div>;
  const write = async () => {
    const files = d.files.filter((f) => choice[f.path] && choice[f.path] !== "skip")
      .map((f) => [f.path, choice[f.path], f.language ?? null]);
    const required = Object.entries(secrets).filter(([, k]) => k === "required").map(([n]) => n);
    const optional = Object.entries(secrets).filter(([, k]) => k === "optional").map(([n]) => n);
    try {
      project.value = await invoke<ProjectView>("project_init", {
        id: m.id, choice: { files, required, optional, requirements: d.requirements ?? null },
      });
      addActivity("Made the folder a project (wrote logos.toml)");
      showToast("Wrote logos.toml");
      refreshProject();
    } catch (e) { showToast(errorText(e)); }
  };
  return (
    <div class="doc-page">
      <h2>Make this folder a project</h2>
      <p>Logos read <span class="mono">{d.model}</span> and its folder. Check the role of each file, then write <span class="mono">logos.toml</span>. The model file isn't changed, and the folder still opens in NetLogo.</p>
      <table class="grid-table">
        <thead><tr><th>File</th><th>Role</th><th>Found by</th></tr></thead>
        <tbody>
          {d.files.map((f) => (
            <tr>
              <td class="mono">{f.path}{!f.exists && <span class="muted"> (not yet created)</span>}</td>
              <td>
                <select aria-label={`Role of ${f.path}`} value={choice[f.path]} onChange={(e) => setChoice({ ...choice, [f.path]: (e.target as HTMLSelectElement).value })}>
                  {ROLES.map((r) => <option value={r}>{r}</option>)}
                  <option value="skip">leave out</option>
                </select>
              </td>
              <td class="muted">{f.reasons.join("; ")}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {d.dynamic.length > 0 && (
        <p class="lvl-warning">The model builds some file names while it runs, so Logos can't list them: {d.dynamic.map(([p, l]) => `${p} on line ${l}`).join(", ")}. Add those files later if the model reads them.</p>
      )}
      {d.skippedFolders.length > 0 && (
        <p class="muted">Not scanned (many files): {d.skippedFolders.map(([n, c]) => `${n} (${c} files)`).join(", ")}.</p>
      )}
      <h3>Secrets</h3>
      {d.secretNames.length === 0 ? <p class="muted">The model's Python code and .env templates name no environment variables.</p> : (
        <table class="grid-table">
          <thead><tr><th>Name</th><th>A run needs it</th></tr></thead>
          <tbody>
            {d.secretNames.map((n) => (
              <tr><td class="mono">{n}</td><td>
                <select aria-label={`Is ${n} required`} value={secrets[n]} onChange={(e) => setSecrets({ ...secrets, [n]: (e.target as HTMLSelectElement).value })}>
                  <option value="required">required for every run</option>
                  <option value="optional">optional</option>
                  <option value="skip">leave out</option>
                </select>
              </td></tr>
            ))}
          </tbody>
        </table>
      )}
      <h3>Python</h3>
      <p class="muted">{d.python ? `Interpreter found: ${d.python}.` : m.code.includes("py:") ? "The model uses the py extension, but no Python interpreter was found." : "The model doesn't use the py extension."}
        {d.requirements ? ` Requirements: ${d.requirements}.` : ""}</p>
      <div class="actions">
        <button class="btn primary" onClick={write}>Write logos.toml</button>
        <button class="btn" onClick={() => closeDoc("project")}>Cancel</button>
      </div>
    </div>
  );
}

// ---- secrets (design 3.7) -----------------------------------------------------------------

interface SecretRow { name: string; required: boolean; declared: boolean; source?: string | null; masked?: string | null }
interface SecretsView { store: string; needsPassphrase: boolean; rows: SecretRow[]; projectId?: string | null }

export function SecretsDoc() {
  const m = model.value!;
  const [v, setV] = useState<SecretsView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<{ name: string; value: string; scope: string; isNew: boolean } | null>(null);
  const [shown, setShown] = useState<{ name: string; value: string; until: number } | null>(null);
  const [copied, setCopied] = useState<{ name: string; until: number } | null>(null);
  const [now, setNow] = useState(Date.now());
  const [recent, setRecent] = useState<string[]>([]);
  const [pass, setPass] = useState("");
  const load = () => {
    invoke<SecretsView>("secrets_list", { id: m.id }).then(setV).catch((e) => setError(errorText(e)));
    invoke<string[]>("secrets_recent").then(setRecent).catch(() => {});
  };
  useEffect(load, [m.id]);
  useEffect(() => {
    const t = setInterval(() => {
      const n = Date.now();
      setNow(n);
      if (shown && n >= shown.until) setShown(null);
      if (copied && n >= copied.until) setCopied(null);
    }, 250);
    return () => clearInterval(t);
  }, [shown, copied]);
  if (error) return <div class="doc-page"><p class="err">{error}</p></div>;
  if (!v) return <div class="doc-page"><p class="muted">Opening the credential store…</p></div>;
  if (v.needsPassphrase) {
    return (
      <div class="doc-page">
        <h2>Secrets</h2>
        <p>This computer has no credential store Logos can use, so secrets are kept in a file encrypted with a passphrase. Type it to open the file for this session.</p>
        <form class="actions" onSubmit={async (e) => {
          e.preventDefault();
          try { setV(await invoke<SecretsView>("secrets_unlock", { id: m.id, passphrase: pass })); setPass(""); }
          catch (er) { showToast(errorText(er)); }
        }}>
          <input type="password" aria-label="Passphrase" value={pass} onInput={(e) => setPass((e.target as HTMLInputElement).value)} autoFocus />
          <button class="btn primary" type="submit">Open</button>
        </form>
      </div>
    );
  }
  const save = async () => {
    if (!editing) return;
    try {
      await invoke("secret_set", { id: m.id, name: editing.name.trim(), value: editing.value, scope: editing.scope });
      addActivity(`Set secret ${editing.name.trim()}`);
      setEditing(null);
      load();
    } catch (e) { showToast(errorText(e)); }
  };
  const remove = async (r: SecretRow) => {
    const scope = r.source === "project" ? "project" : "user";
    if (!(await ask(`Delete the stored value of ${r.name}? Runs that need it will stop until it is set again.`, { title: "Delete secret", kind: "warning", okLabel: "Delete" }))) return;
    try { await invoke("secret_delete", { id: m.id, name: r.name, scope }); addActivity(`Deleted secret ${r.name}`); load(); }
    catch (e) { showToast(errorText(e)); }
  };
  const unmask = async (r: SecretRow) => {
    // The operating system's confirmation (Windows Hello, Touch ID) comes with step 15; until
    // then Logos asks here.
    if (!(await ask(`Show the value of ${r.name} for 30 seconds? Logos records that it was shown, never the value.`, { title: "Unmask secret", kind: "warning", okLabel: "Show" }))) return;
    try {
      const value = await invoke<string>("secret_unmask", { id: m.id, name: r.name });
      setShown({ name: r.name, value, until: Date.now() + 30_000 });
      load();
    } catch (e) { showToast(errorText(e)); }
  };
  const copy = async (r: SecretRow) => {
    try {
      await invoke("secret_copy", { id: m.id, name: r.name });
      setCopied({ name: r.name, until: Date.now() + 30_000 });
      load();
    } catch (e) { showToast(errorText(e)); }
  };
  const secs = (until: number) => Math.max(0, Math.ceil((until - now) / 1000));
  return (
    <div class="doc-page">
      <h2>Secrets</h2>
      <p class="muted">Stored in {v.store}. Values never go into project files, run records or logs.</p>
      <table class="grid-table" aria-label="Secrets">
        <thead><tr><th>Name</th><th>Needed</th><th>State</th><th>Value</th><th>Actions</th></tr></thead>
        <tbody>
          {v.rows.map((r) => (
            <tr>
              <td class="mono">{r.name}</td>
              <td>{r.declared ? (r.required ? "required" : "optional") : <span class="muted">not declared</span>}</td>
              <td>{r.source ? <span class="ok">set ({r.source})</span> : <span class={r.required ? "err" : "muted"}><strong>{r.required ? "✕ missing" : "missing"}</strong></span>}</td>
              <td class="mono" aria-live="polite">
                {shown?.name === r.name ? <span>{shown.value} <span class="muted">(hides in {secs(shown.until)} s)</span></span> : r.masked ?? ""}
                {copied?.name === r.name && <span class="muted"> copied; clipboard clears in {secs(copied.until)} s</span>}
              </td>
              <td>
                <div class="actions">
                  <button class="btn small" onClick={() => setEditing({ name: r.name, value: "", scope: r.source === "project" ? "project" : "user", isNew: false })}>{r.source ? "Change" : "Set"}</button>
                  {r.source && r.source !== "environment" && <button class="btn small" onClick={() => unmask(r)}>Unmask</button>}
                  {r.source && r.source !== "environment" && <button class="btn small" onClick={() => copy(r)}>Copy</button>}
                  {r.source && r.source !== "environment" && <button class="btn small" onClick={() => remove(r)}>Delete</button>}
                </div>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <div class="actions" style="margin-top: 8px">
        <button class="btn small" onClick={() => setEditing({ name: "", value: "", scope: "user", isNew: true })}><Icon name="add" /> Add a secret</button>
      </div>
      {editing && (
        <form class="card" style="margin-top: 12px" onSubmit={(e) => { e.preventDefault(); save(); }}>
          <h3 style="margin-top: 0">{editing.isNew ? "Add a secret" : `Set ${editing.name}`}</h3>
          <div class="actions">
            {editing.isNew && <label>Name <input type="text" class="mono" value={editing.name} onInput={(e) => setEditing({ ...editing, name: (e.target as HTMLInputElement).value })} /></label>}
            <label>Value <input type="password" autoComplete="off" value={editing.value} autoFocus onInput={(e) => setEditing({ ...editing, value: (e.target as HTMLInputElement).value })} /></label>
            <label>Keep for <select value={editing.scope} onChange={(e) => setEditing({ ...editing, scope: (e.target as HTMLSelectElement).value })}>
              <option value="user">all my projects</option>
              <option value="project">this project only</option>
            </select></label>
            <button class="btn primary" type="submit">Save</button>
            <button class="btn" type="button" onClick={() => setEditing(null)}>Cancel</button>
          </div>
        </form>
      )}
      <p class="muted" style="margin-top: 14px">The assistant, the API and the MCP server (Phase 3) can see names and whether each is set, never values.</p>
      {recent.length > 0 && (
        <>
          <h3>Recently shown or copied</h3>
          <ul class="findings">{recent.map((l) => <li class="mono">{l}</li>)}</ul>
        </>
      )}
    </div>
  );
}
