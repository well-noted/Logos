//! Editor support for NetLogo code (Phase 1, Part S): the analysis behind editor features,
//! diagnostics, and the language server (`logos lsp`).

pub mod analysis;
pub mod diagnostics;
pub mod program;
pub mod server;
pub mod text;

pub use diagnostics::{Diagnostic, Origin, diagnostics};
