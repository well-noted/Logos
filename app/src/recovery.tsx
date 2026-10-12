// Autosave and crash recovery (D-24), and models changed on disk by other programs (spec
// part I). The host copies unsaved models to the app data folder every 30 seconds; this
// module adds a copy when the window loses focus, offers an autosaved copy when a model is
// opened, and reacts to changes on disk.

import { signal } from "@preact/signals";
import { useEffect, useRef } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ask } from "@tauri-apps/plugin-dialog";
import type { ModelView } from "./ipc";
import { errorText } from "./ipc";
import { replaceOpenCode } from "./code";
import { addActivity, dirty, model, notApplied, showToast } from "./state";

interface DiffLine { kind: "same" | "added" | "removed" | "skipped"; text: string; line: number }
interface Recovery { savedAt: string; diff: DiffLine[]; added: number; removed: number }

const offered = signal<{ id: number; name: string; r: Recovery } | null>(null);

/** Puts a model view from the host in place of the open model's code, info and widgets. */
function takeView(v: ModelView) {
  const m = model.value;
  if (!m || m.id !== v.id) return;
  model.value = { ...m, code: v.code, info: v.info, widgets: v.widgets };
  replaceOpenCode(v.code);
  dirty.value = v.dirty;
  if (v.code !== m.code) notApplied.value = true;
}

/** After a model opens: is there an autosaved copy from a session that ended unsaved? */
export async function checkRecovery(id: number, name: string) {
  try {
    const r = await invoke<Recovery | null>("model_recovery", { id });
    if (r) offered.value = { id, name, r };
  } catch (e) {
    showToast(`Can't read the autosaved copy: ${errorText(e)}`);
  }
}

async function resolve(restore: boolean) {
  const o = offered.value;
  offered.value = null;
  if (!o) return;
  try {
    const v = await invoke<ModelView | null>("model_resolve_recovery", { id: o.id, restore });
    if (v) {
      takeView(v);
      addActivity(`Restored unsaved edits to ${o.name} from ${o.r.savedAt}`);
      showToast("Restored. The edits are unsaved; save to keep them.");
    } else {
      addActivity(`Discarded the autosaved copy of ${o.name}`);
    }
  } catch (e) {
    showToast(errorText(e));
  }
}

export function RecoveryDialog() {
  const o = offered.value;
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => { ref.current?.querySelector<HTMLElement>("button.primary")?.focus(); }, [o?.id]);
  if (!o) return null;
  const when = o.r.savedAt ? new Date(o.r.savedAt).toLocaleString() : "an earlier session";
  return (
    <div class="overlay">
      <div class="settings recovery" role="dialog" aria-modal="true" aria-labelledby="rec-h" ref={ref}
        onKeyDown={(e) => { if (e.key === "Escape") resolve(false); }}>
        <header><h2 id="rec-h">Restore unsaved edits to {o.name}?</h2></header>
        <div class="body">
          <p style="margin: 0">
            Logos stopped before these edits were saved. The autosaved copy is from {when}: {o.r.added} line{o.r.added === 1 ? "" : "s"} added
            and {o.r.removed} removed compared with the file on disk.
          </p>
          <pre class="diff" aria-label="Changes in the autosaved copy">
            {o.r.diff.map((l) => (
              <div class={`d-${l.kind}`}>
                <span class="ln">{l.kind === "skipped" ? "" : l.line}</span>
                <span class="sign" aria-hidden="true">{l.kind === "added" ? "+" : l.kind === "removed" ? "−" : " "}</span>
                <span class="sr-only">{l.kind === "added" ? "added: " : l.kind === "removed" ? "removed: " : ""}</span>
                {l.text}
              </div>
            ))}
          </pre>
          <div class="actions" style="justify-content: flex-end">
            <button class="btn" onClick={() => resolve(false)}>Discard the copy</button>
            <button class="btn primary" onClick={() => resolve(true)}>Restore</button>
          </div>
        </div>
      </div>
    </div>
  );
}

interface DiskChange { id: number; reloaded: boolean; missing: boolean }

/** Listens for models changed on disk, and autosaves when the window loses focus. */
export function installRecovery() {
  window.addEventListener("blur", () => { invoke("autosave_now").catch(() => {}); });
  listen<DiskChange>("disk-change", async (e) => {
    const c = e.payload;
    const m = model.value;
    if (!m || m.id !== c.id) return;
    if (c.missing) {
      showToast(`${m.name} was deleted or moved on disk. Save writes it again.`);
      addActivity(`${m.name} was deleted or moved on disk`);
      return;
    }
    if (c.reloaded) {
      try {
        takeView(await invoke<ModelView>("model_reload", { id: m.id }));
        showToast(`${m.name} changed on disk and was reloaded. Apply to use it in the running model.`);
        addActivity(`Reloaded ${m.name} after a change on disk`);
      } catch (err) { showToast(errorText(err)); }
      return;
    }
    const takeDisk = await ask(
      `${m.name} was changed by another program, and it has edits here that aren't saved.\n\nLoad the file from disk (your unsaved edits are lost), or keep your edits (saving then replaces the other program's changes)?`,
      { title: "Model changed on disk", kind: "warning", okLabel: "Load from disk", cancelLabel: "Keep my edits" },
    );
    if (takeDisk) {
      try {
        takeView(await invoke<ModelView>("model_reload", { id: m.id }));
        addActivity(`Loaded ${m.name} from disk, dropping unsaved edits`);
      } catch (err) { showToast(errorText(err)); }
    } else {
      addActivity(`Kept unsaved edits to ${m.name} over a change on disk`);
    }
  });
}
