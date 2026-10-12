// The Info tab (design 2.4): the model's documentation as rendered Markdown, with an Edit
// toggle that shows the Markdown beside a live preview, as NetLogo does.

import { useState } from "preact/hooks";
import { Marked } from "marked";
import { model, setInfo } from "./state";
import { Icon } from "./icons";

/** Rendered Markdown without raw HTML from the file: the Info text comes from the model, so
 * HTML in it is shown as text. */
const escapeHtml = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

export const safeMarkdown = new Marked({
  gfm: true,
  renderer: { html: (token: { text: string }) => escapeHtml(token.text) },
});

export function render(md: string): string {
  return safeMarkdown.parse(md, { async: false }) as string;
}

export function InfoTab() {
  const m = model.value;
  const [editing, setEditing] = useState(false);
  if (!m) return null;
  return (
    <div class="split">
      <div class="code-bar" role="toolbar" aria-label="Info">
        <button class="btn small" aria-pressed={editing} onClick={() => setEditing(!editing)}>
          <Icon name="edit" /> {editing ? "Done" : "Edit"}
        </button>
      </div>
      <div class="info-host">
        {editing ? (
          <div class="info-edit">
            <textarea aria-label="Info text (Markdown)" value={m.info}
              onInput={(e) => setInfo((e.target as HTMLTextAreaElement).value)} />
            <div class="info-render" style="overflow:auto" dangerouslySetInnerHTML={{ __html: render(m.info) }} />
          </div>
        ) : m.info.trim() ? (
          <article class="info-render" dangerouslySetInnerHTML={{ __html: render(m.info) }} />
        ) : (
          <p class="empty-note">This model has no Info text yet. Press Edit to write some.</p>
        )}
      </div>
    </div>
  );
}
