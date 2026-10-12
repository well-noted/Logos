# Logos desktop app: front end

The front end of the Logos desktop app (Phase 2, `docs/phase-2-spec.md`). The host is `crates/logos-app`, a Tauri 2 program that embeds what this folder builds into `app/dist`.

- TypeScript with Preact and Preact signals for the interface, CodeMirror 6 for the code editor, `marked` for Markdown, IBM Plex fonts (bundled, SIL Open Font License). Every dependency is MIT, Apache-2.0 or OFL; `npm ls --all` lists them.
- No server and no network port. The host talks to the front end through Tauri commands, events, and a Tauri channel for view frames (spike F1).

## Building

Needs Node.js 22 or later (on Windows: `winget install OpenJS.NodeJS.LTS`).

```
cd app
npm ci
npm run build        # writes app/dist
cd ..
cargo run -p logos-app --release
```

`npm run watch` rebuilds on every change; rebuild the host to pick it up (Tauri embeds `app/dist` at compile time). `npm run check` type-checks; `node build.mjs --test` runs the front-end tests (theme contrast, A4).

Without `app/dist`, `cargo build` still works and the app shows a page asking for the front end to be built, so `cargo test --workspace` doesn't need Node.

## Opening a model from the command line

`logos-app path\to\model.nlogox` opens the model once NetLogo is ready.

## Working without NetLogo

`dev/mock-bridge.py` stands in for the JVM bridge so the screens can be worked on where NetLogo isn't installed (for example in a cloud session). It speaks enough of protocol 1 to open a model, run commands (also during a run), send snapshots, stream a pretend view, change a run's speed and stop condition, and answer monitors. It has no plots and its Apply removes nothing. It does not run NetLogo code, and no test of NetLogo behavior may use it.

```
LOGOS_JAVA=app/dev/mock-bridge.py
LOGOS_NETLOGO_HOME=<a folder containing app/netlogo-7.0.4.jar, which may be empty>
LOGOS_BRIDGE_JAR=<any existing file>
```

## Where things are

| File | What |
|---|---|
| `src/app.tsx` | The window: menus, regions, model document |
| `src/launcher.tsx` | Start screen and first launch: finding NetLogo, the engine check |
| `src/interface.tsx` | Interface tab: toolbar, run strip, widgets, the view, pop-out view |
| `src/code.tsx`, `src/info.tsx` | Code and Info tabs |
| `src/editor.tsx` | Interface edit mode: moving, resizing, adding and deleting widgets, property sheets, world settings, undo |
| `src/inspector.tsx` | Agent inspector, watches, pause when |
| `src/search.ts`, `src/searchpanel.tsx` | Search and replace across code, Info and widgets |
| `src/recovery.tsx` | Autosave on focus loss, restoring an autosaved copy, models changed on disk |
| `src/project.tsx` | Files activity, project setup and secrets |
| `src/data.tsx` | Data file preview and data dictionary editor |
| `src/experiments.tsx`, `src/bspace.ts` | The experiment editor, the run dashboard and results |
| `src/panels.tsx` | Activity bar, outline, bottom panel (Command Center, Output, Problems, Log, Activity), status bar |
| `src/settings.tsx`, `src/themes.ts` | Settings, built-in and custom themes (D-32) |
| `src/netlogo.ts` | NetLogo names by class (from the primitive table and the model), for highlighting |
| `src/menu.tsx`, `src/dialog.tsx` | Context menus; confirmations with "Don't ask again" |
| `src/diff.tsx` | Comparing the model's code with the saved file, git HEAD or another model |
| `src/snap.ts` | Shift-drag snapping in interface edit mode |
| `src/commands.ts`, `src/palette.tsx` | Every action, keyboard shortcuts, command palette |
| `src/state.ts`, `src/ipc.ts`, `src/frames.ts` | App state, calls into the host, view frames |
