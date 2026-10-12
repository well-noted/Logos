# Roadmap

Where Logos is, phase by phase. Each phase has a specification with success criteria and ends with a results file. Phase 2 to 4 are outlines; each gets a full specification when the phase before it is done.


| Phase | Content | Status |
|---|---|---|
| 0 | Technical spikes: JVM bridge and live view (A1-A12), lossless file format and parser (B1-B7) | Done, 2026-10-11 ([results](phase-0-results.md)) |
| 1 | Core: projects and the files around a model, experiments with Parquet output and provenance, secrets, full NetLogo parser, language server, `logos` CLI | In progress ([spec](phase-1-spec.md), [results](phase-1-results.md)) |
| 2 | Programmatic access and interface model: local API service, Python client and MCP server sharing one capability layer (D-03), so coding assistants and other agents can drive Logos; analysis through Python (D-08); experiment designs beyond BehaviorSpace; interface editor logic and widget layout (D-18); a Logos LLM bridge for models (below) | Outline |
| 3 | Desktop app on Windows, macOS and Linux: code editor on the language server, interface editor, live view from rendered frames and agent state (D-10), experiment dashboard, in-app assistant with Anthropic, OpenAI and Ollama through their APIs or through MCP (D-04), Jupyter notebooks (D-16), recording, replay and budgets for models that call language models (Phase 1 Part L, moved here by D-20) | Outline |
| 4 | Native engine for models that don't need NetLogo-only features, checked against NetLogo 7.0.4 by the conformance suite; `gis` and `nw` stay on the reference engine (D-12). Any model made for the native engine can be exported to `.nlogox` for NetLogo, with a report of anything that NetLogo can't run and was disabled (D-19) | Outline |

## A Logos LLM bridge for models (Phase 2)

Many modelers want agents that call language models (GABM) but don't write Python. MASTOC-LLM shows the pattern that works in NetLogo today: the `py` extension loads a Python module that calls the providers. Logos will ship that pattern ready to use, in a form NetLogo can open:

- A NetLogo include file (for example `logos-llm.nls`) with procedures such as asking a model a question, asking it to choose from a list of options, and passing messages between agents, written in NetLogo and calling the Python module through `py`.
- The Python module next to it, handling providers (Anthropic, OpenAI-compatible endpoints, Ollama), memory per agent, and response parsing.
- The bridge's interactions are set up in a graphical editor in Logos: which provider and model each agent type uses, prompts and the model variables they include, memory, messages between agents, and how responses become NetLogo values. The user never has to edit the Python module; Logos writes its settings to a file the module reads.
- Logos adds both to a project on request, lists them in `logos.toml`, and keeps the keys in the credential store (Part K). The model then also runs in plain NetLogo with the `py` extension and Python installed (D-19).
- Runs are recorded, replayable and budgeted by Part L (Phase 3, D-20) with no extra work.

## Size

Phase 0 was 16 PR-sized steps, done 2026-10-09 to 2026-10-11, with Thomas running each Windows check. Phase 1 lists 18 steps, and most of them are larger than Phase 0's: the BehaviorSpace runner, the recording proxy, the expression parser and the language server are each comparable to all of Spike B. A fair expectation is three to four times the work of Phase 0. The order inside Phase 1 lets Thomas use `logos run` on his own models after step 8, before the LLM and language work.
