// Entry point of the Logos front end (Phase 2). The host is crates/logos-app.

import "@fontsource/ibm-plex-sans/latin-400.css";
import "@fontsource/ibm-plex-sans/latin-500.css";
import "@fontsource/ibm-plex-sans/latin-600.css";
import "@fontsource/ibm-plex-mono/latin-400.css";
import "./styles.css";
import { render } from "preact";
import { App } from "./app";
import { installShortcuts } from "./commands";
import { interceptLinks } from "./links";
import { init, refreshTheme } from "./state";

import { invoke } from "@tauri-apps/api/core";

const uiLog = (level: string, text: string) => { invoke("ui_log", { level, text }).catch(() => {}); };
window.addEventListener("error", (e) => uiLog("error", `${e.message} at ${e.filename}:${e.lineno}`));
window.addEventListener("unhandledrejection", (e) => uiLog("error", `unhandled: ${JSON.stringify(e.reason)}`));

refreshTheme();
installShortcuts();
interceptLinks();
init().catch((e) => console.error(e));
render(<App />, document.getElementById("app")!);
