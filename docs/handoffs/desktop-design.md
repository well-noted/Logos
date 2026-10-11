# Handoff: design the Logos desktop app

You are joining Logos, an IDE for agent-based models written in NetLogo, built by Thomas Tuoti. Your task is the **design** of the desktop app (Phase 3 in `docs/roadmap.md`), done now so it is ready when Phase 3 starts. You are not writing app code. Other instances are building Phase 1 (the core) in parallel; your design must fit what they build.

## How to work with Thomas

- Treat the work as a collaboration. When you need a source (an article, a design reference, a document), ask Thomas for it rather than working from memory.
- Ask before deciding anything that is his to decide. Propose options with their tradeoffs, recommend one, and say why.
- Write plainly, in American English: direct statements, no metaphor or flourish, no "not X, but Y" constructions, no short punchy line at the end of every paragraph.
- Don't add co-author or session lines to commits or PRs.
- The repository is at `C:\Users\tetuo\Logos` on his Windows machine (GitHub-ready, public, MIT OR Apache-2.0 except `bridge/`, which is GPL-3.0). Its documents are also in the claude.ai Project "Logos".

## Read first

In the Project or the repository:

1. `docs/roadmap.md`: phases and where the desktop app fits.
2. `docs/decisions.md`: decisions D-01 to D-19. Accepted decisions are settled; don't reopen them without a reason.
3. `docs/phase-1-spec.md`: the core the app sits on (projects, secrets, experiments, LLM recording, language server).
4. `protocol/PROTOCOL.md`: how Logos talks to NetLogo (the bridge), including the live view frames.
5. `docs/phase-0-results.md`: measured performance (frame rates, run speeds) that the design can rely on.

## What Logos is

NetLogo is the standard teaching environment for agent-based modeling. Logos lets a modeler take a NetLogo model from a teaching exercise to a research study without rewriting it. It reads and writes NetLogo's `.nlogox` files without losing anything, runs models on NetLogo 7.0.4 itself through a JVM bridge, and adds what research needs: projects with their data and code files, parallel experiments with provenance, recorded and replayable LLM calls for generative agent-based models (GABM), and an editor with modern language features.

Users write NetLogo code with NetLogo's primitives and extensions. A model made or edited in Logos opens in NetLogo unchanged, and the reverse (D-19). Anything Logos adds lives in separate files next to the model.

## Decided: architecture and constraints

- **Platforms:** Windows, macOS and Linux. Windows is the primary platform and is tested first. Paths can contain spaces (D-17).
- **Core:** Rust. The desktop app is a front end over Rust crates that already exist or are being built: `logos-nlogox` (lossless model files), `logos-syntax` (lossless parser), `logos-project` (`logos.toml`, file detection, checks), `logos-experiment` (BehaviorSpace runner), `logos-bridge-client` (talks to the JVM bridge), and in Phase 1 a language server (`logos lsp`) and the `logos` command.
- **Engine:** NetLogo 7.0.4 in a separate JVM process (the bridge), one process with many workspaces (D-05, D-09). Each project records the engine version; the app warns before running on a different one.
- **UI framework: not chosen.** Phase 0 deliberately avoided choosing; its throwaway viewer used `winit` and `softbuffer`. Recommending a framework is part of your task (see below).
- **Licensing:** the app is MIT OR Apache-2.0. Nothing from NetLogo's source may be copied outside `bridge/` (D-01).

## Decided: what the app must contain

### NetLogo's own parts

A NetLogo model has three tabs: Interface, Info and Code. Logos keeps all three, editable, and round-trips them through `.nlogox` without loss.

- **Interface:** NetLogo's widgets: view, button, slider, switch, chooser, input box, monitor, plot (with pens), output, note. Editing must keep every attribute NetLogo writes (Spike B). The interface editor gets a command that lays out widgets from models saved before NetLogo 6.4 for the new widget sizes, once Logos knows its own widget sizes (D-18).
- **Info:** the model's documentation, Markdown as NetLogo uses it.
- **Code:** the NetLogo code and its `.nls` include files.

### Live view (D-10)

- The view shows NetLogo's own rendering, streamed as RGBA frames from the bridge. Measured on Thomas's machine: 28.7 frames per second shown at a 30 fps cap for AA4-Combined's 320 × 480 view.
- Agent state is streamed separately: patch colors, and per turtle `who`, position, heading, color, size, shape, breed and hidden flag. 30 updates per second at a 30 fps cap with 10,000 turtles; 219 per second uncapped. This drives inspection (click an agent, see its variables), interaction, and views NetLogo can't draw.
- Run controls: setup, go (once and forever), stop, speed. Reading values never changes a run (D-14); the app must use the bridge's observation path for monitors and inspectors.

### Projects (Phase 1, Part P)

A project is a folder with one `.nlogox` and the files it uses, described in `logos.toml`: input and output data files with optional data dictionaries (columns, types, units, sources), Python modules, config files, the Python requirements file, and the names of required and optional secrets. `logos project init` proposes this list by reading the model's code and folder; the user confirms. `logos check` reports what would stop a run. The app needs views for all of this: a file explorer that shows each file's role, a data-dictionary editor, and the check results.

Legacy `.nlogo` files: the app warns, then converts to `.nlogox` through NetLogo, never overwriting the original (D-06, D-18).

### Secrets (Phase 1, Part K)

API keys and environment variables are set from inside Logos and stored in the operating system's credential store (Windows Credential Manager, macOS Keychain, Linux Secret Service). Rules the design must follow:

- Values are never written to project files, run records, logs or crash reports.
- Values are shown masked. Unmasking on request first asks the operating system to confirm the user (Windows Hello, Touch ID or account password, the keyring prompt) where available, otherwise asks in Logos. The value hides again after 30 seconds; a copied value is cleared from the clipboard after 30 seconds. Each unmask is logged by name and time.
- Unmasking is never available to the API, the MCP server or the in-app assistant.
- A `.env` file can be imported once.

### Experiments and provenance (Phase 1, Part E)

BehaviorSpace experiments run on parallel workspaces, with results in Parquet plus NetLogo's own table and spreadsheet CSV, and a run record per run (engine version, model and input file hashes, seed, parameters, timing, how the run ended). Interrupted experiments resume. The app needs: an experiment editor (all BehaviorSpace settings), a run dashboard (progress per worker, runs done and failed, time remaining), result browsing with simple plots, and the run records.

### Models that call language models (Phase 1, Part L; Phase 2 bridge)

- Every LLM call a model makes during a run is recorded: request (authentication removed), response, provider, model, settings, tokens, timing, run and step. Reasoning content is recorded when the provider returns it; the record says when it was summarized, redacted or withheld, which some providers do.
- Recorded runs can be replayed exactly without network access. Experiments can have token, cost and rate budgets.
- **Logos LLM bridge (Phase 2):** Logos ships a ready-made bridge (a NetLogo include file plus a Python module) so modelers can add LLM-driven agents without writing Python, following the pattern of Thomas's MASTOC-LLM model. The bridge interactions are set up in a **graphical editor** (which provider and model each agent type uses, prompts and their variables, memory length, message passing between agents, response parsing into NetLogo values) without editing the Python file. The model still runs in plain NetLogo with the `py` extension.
- The app needs views for: the bridge editor; a call log browser per run (prompt, response, reasoning, tokens, cost, filterable by agent and step); replay controls; budget settings and usage.

### Assistant and agents (D-03, D-04)

- An in-app assistant using Anthropic, OpenAI or Ollama, with the user's own keys, through the providers' APIs or through MCP.
- Logos exposes an MCP server and a local API (Phase 2) with the same capabilities, so external coding assistants and agents can open projects, edit code, run experiments and read results. The design should show how a user sees and controls what an assistant or external agent is doing (which project, which actions, approvals for actions that change files or spend money).

### Notebooks (D-16, proposed)

Jupyter notebooks (`.ipynb`) open inside Logos on a standard `ipykernel` with the Logos Python client loaded, for analysis next to the model. They stay ordinary files that open in Jupyter or VS Code.

### Language support (Phase 1, Part S)

From the language server: diagnostics from NetLogo's own compiler (D-07), outline, go to definition, find references, rename, hover with primitive syntax, completion, and word-wise editing that treats hyphenated names (`turtles-own`) as words with parts.

### Standard IDE tools

Thomas wants the tools users expect from modern IDEs. At least:

- File explorer, tabs, split editors, and a project switcher with recent projects.
- Command palette, keyboard shortcuts (rebindable), and settings (per user and per project).
- Search and replace across the project, including `.nls` and Python files.
- A problems panel (compile errors, check results) and an output and log panel.
- An integrated terminal.
- Git: status, diff (including a readable diff for `.nlogox` widget and code changes), commit, branches, history.
- Light and dark themes, adjustable font sizes, and accessibility: full keyboard use and screen reader labels.
- Undo and redo across code and interface edits.
- Auto-save and crash recovery.
- A debugging workflow suited to agent-based models: step by tick, pause on a condition, inspect agents and patches, watch reporters over time.

## What to produce

Put your work in `docs/desktop-design.md` (and supporting files under `docs/desktop-design/` if needed), and in the Project.

1. **Information architecture:** the app's main areas, panels and how they relate; what a project window contains.
2. **Layouts:** wireframes for the main screens, as simple diagrams (SVG or Mermaid in Markdown, or an artifact Thomas can open). At least: the model screen (code, interface, info, live view together), the experiment editor and run dashboard, the LLM bridge editor, the LLM call log, secrets, the assistant panel with its approvals, and first launch (finding NetLogo, opening or converting a model).
3. **Key flows**, step by step: open a NetLogo model and make it a project; edit code and see NetLogo's errors; edit the interface and run with the live view; inspect an agent; set up and run an experiment and look at results; add LLM-driven agents with the bridge editor, run, and replay; set and unmask a secret; let an assistant change the model with approvals.
4. **UI framework recommendation:** two or three candidates suited to a Rust core on Windows, macOS and Linux (for example a web-view shell such as Tauri, or native Rust toolkits such as egui, iced or Slint), compared on: rendering the live view at 30 fps or more, embedding a code editor with language-server support, accessibility, look on each platform, and how much of the editor must be built rather than reused. Recommend one; Thomas decides.
5. **Open questions** for Thomas, each with options.

Keep the design consistent with the decisions above. Where a feature needs something the core doesn't provide yet, list it so it can go into the Phase 1 or Phase 2 specifications.
