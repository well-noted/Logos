// The Code tab's language server client (design 4.2). The server runs inside the host; this
// sends its messages through a Tauri command and receives the replies as events. The host
// turns the model's code into the whole .nlogox file and back (crates/logos-app/src/lsp.rs).

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { LSPClient, languageServerExtensions, type Transport } from "@codemirror/lsp-client";
import { problems, type Problem } from "./state";

const handlers = new Set<(value: string) => void>();
let listening = false;

const transport: Transport = {
  send(message: string) {
    invoke("lsp_send", { message }).catch(() => {});
  },
  subscribe(handler) { handlers.add(handler); },
  unsubscribe(handler) { handlers.delete(handler); },
};

let client: LSPClient | null = null;

/** Diagnostics for the code, kept for the Problems panel. */
function onDiagnostics(params: { uri: string; diagnostics: { message: string; range: { start: { line: number } } ; severity?: number }[] }) {
  const fromCode: Problem[] = params.diagnostics.map((d) => ({
    text: `${d.message} (line ${d.range.start.line + 1})`,
    source: "NetLogo",
  }));
  problems.value = [...problems.value.filter((p) => p.source !== "NetLogo"), ...fromCode];
}

export function lspClient(): LSPClient {
  if (!listening) {
    listening = true;
    listen<string>("lsp", (e) => { for (const h of handlers) h(e.payload); });
  }
  if (!client) {
    client = new LSPClient({
      timeout: 10000,
      extensions: languageServerExtensions(),
      notificationHandlers: {
        "textDocument/publishDiagnostics": (_c, params) => { onDiagnostics(params); return false; },
      },
    }).connect(transport);
  }
  return client;
}
