# Logos

Logos is a development environment for agent-based models written in NetLogo. It reads and writes NetLogo's `.nlogox` format without loss and runs models on the real NetLogo engine, so a model written for teaching can be extended into a research study without being rewritten.

Status: Phase 0 (technical spikes). Nothing is usable yet.

## Documents

- [Decision log](docs/decisions.md)
- [Repository layout and Windows build](docs/repo-layout.md)
- [Phase 0 specification](docs/phase-0-spec.md)

## License

The Rust code, protocol, Python client and documentation are dual-licensed under either of

- MIT license ([LICENSE-MIT](LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option.

The JVM bridge in `bridge/` runs inside the same process as NetLogo and is licensed GPL-3.0-or-later. See [decision D-01](docs/decisions.md).

Models under `corpus/` keep their own licenses, stated in each model's folder.

NetLogo is copyright Uri Wilensky and is distributed under the GNU General Public License. Logos is not affiliated with the NetLogo project or Northwestern University.
