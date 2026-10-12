//! Editor support for NetLogo code (Phase 1, Part S): diagnostics, and next the language
//! server (`logos lsp`).

pub mod diagnostics;
pub mod text;

pub use diagnostics::{Diagnostic, Origin, diagnostics};
