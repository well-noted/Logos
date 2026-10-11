# Roadmap

Where Logos is, phase by phase. Each phase has a specification with success criteria and ends with a results file. Phase 2 to 4 are outlines; each gets a full specification when the phase before it is done.

Phases 2 to 4 here are put together from the decision log (D-03, D-04, D-08, D-12, D-16, D-18). They should be checked against the original design brief.

| Phase | Content | Status |
|---|---|---|
| 0 | Technical spikes: JVM bridge and live view (A1-A12), lossless file format and parser (B1-B7) | Done, 2026-10-11 ([results](phase-0-results.md)) |
| 1 | Core: projects and the files around a model, experiments with Parquet output and provenance, recording and replay of LLM calls, secrets, full NetLogo parser, language server, `logos` CLI | Specified ([spec](phase-1-spec.md)) |
| 2 | Programmatic access and interface model: local API service, Python client and MCP server sharing one capability layer (D-03); analysis through Python (D-08); experiment designs beyond BehaviorSpace; interface editor logic and widget layout (D-18) | Outline |
| 3 | Desktop app: code editor on the language server, interface editor, live view from rendered frames and agent state (D-10), experiment dashboard, in-app assistant with Anthropic, OpenAI and Ollama (D-04), Jupyter notebooks (D-16) | Outline |
| 4 | Native engine for models that don't need NetLogo-only features, checked against NetLogo 7.0.4 by the conformance suite; `gis` and `nw` stay on the reference engine (D-12) | Outline |

## Size

Phase 0 was 16 PR-sized steps, done 2026-10-09 to 2026-10-11, with Thomas running each Windows check. Phase 1 lists 18 steps, and most of them are larger than Phase 0's: the BehaviorSpace runner, the recording proxy, the expression parser and the language server are each comparable to all of Spike B. A fair expectation is three to four times the work of Phase 0. The order inside Phase 1 lets Thomas use `logos run` on his own models after step 8, before the LLM and language work.
