// A confirmation dialog drawn by Logos, for questions that offer "Don't ask again" (the
// operating system's dialogs can't hold a checkbox).

import { signal } from "@preact/signals";
import { useEffect, useRef, useState } from "preact/hooks";

interface Request {
  title: string; message: string; okLabel: string; cancelLabel: string; dontAsk: boolean;
  resolve: (r: { ok: boolean; dontAsk: boolean }) => void;
}

const pending = signal<Request | null>(null);

export function confirmDialog(o: { title: string; message: string; okLabel?: string; cancelLabel?: string; dontAsk?: boolean }) {
  return new Promise<{ ok: boolean; dontAsk: boolean }>((resolve) => {
    pending.value = { okLabel: "OK", cancelLabel: "Cancel", dontAsk: false, ...o, resolve };
  });
}

export function ConfirmDialog() {
  const r = pending.value;
  const [dontAsk, setDontAsk] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => { setDontAsk(false); ref.current?.querySelector<HTMLElement>("button.primary")?.focus(); }, [r]);
  if (!r) return null;
  const done = (ok: boolean) => { pending.value = null; r.resolve({ ok, dontAsk: ok && dontAsk }); };
  return (
    <div class="overlay" onKeyDown={(e) => { if (e.key === "Escape") done(false); }}>
      <div class="settings confirm" role="alertdialog" aria-modal="true" aria-labelledby="confirm-title" aria-describedby="confirm-msg" ref={ref}>
        <header><h2 id="confirm-title">{r.title}</h2></header>
        <div class="body">
          <p id="confirm-msg" style="margin: 0; white-space: pre-wrap">{r.message}</p>
          {r.dontAsk && (
            <label><input type="checkbox" checked={dontAsk} onChange={(e) => setDontAsk((e.target as HTMLInputElement).checked)} /> Don't ask again (Settings can turn this back on)</label>
          )}
          <div class="actions" style="justify-content: flex-end">
            <button class="btn" onClick={() => done(false)}>{r.cancelLabel}</button>
            <button class="btn primary" onClick={() => done(true)}>{r.okLabel}</button>
          </div>
        </div>
      </div>
    </div>
  );
}
