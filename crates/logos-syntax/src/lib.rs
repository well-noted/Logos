//! Lossless lexing and structure parsing of NetLogo code (Phase 0, Spike B; D-11).
//!
//! [`parse`] returns a [`rowan`] syntax tree in which every byte of the source belongs to
//! exactly one token, so the tree's text is the source. The parser recognizes NetLogo's
//! top-level structure (declarations and procedures) and, inside procedure bodies, only
//! bracket and parenthesis nesting. It never stops at an error: problems are returned as
//! [`SyntaxError`]s with their spans, and the tree still covers the whole source.
//!
//! Full expression parsing needs the primitive table (`engine.primitives`) and is Phase 1 work.

pub mod ast;
pub mod ident;
pub mod lexer;
mod parser;

use std::ops::Range;

pub use ast::{Breed, Declaration, Procedure, SourceFile, Structure};
pub use ident::segments;
pub use lexer::{LexError, Token, lex};

/// Kinds of tokens and nodes.
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u16)]
pub enum SyntaxKind {
    // Tokens.
    WHITESPACE = 0,
    COMMENT,
    NUMBER,
    STRING,
    IDENT,
    EXTENSION_LITERAL,
    L_BRACKET,
    R_BRACKET,
    L_PAREN,
    R_PAREN,
    L_BRACE,
    R_BRACE,
    COMMA,
    /// A character NetLogo doesn't allow.
    ERROR_TOKEN,

    // Nodes.
    SOURCE_FILE,
    /// `globals [ ... ]`, `turtles-own [ ... ]`, `<breeds>-own [ ... ]`, `extensions [ ... ]`,
    /// `__includes [ ... ]`, `breed [ ... ]`, `directed-link-breed [ ... ]`,
    /// `undirected-link-breed [ ... ]`.
    DECLARATION,
    /// The bracketed list of a declaration.
    NAME_LIST,
    /// `to` / `to-report` ... `end`.
    PROCEDURE,
    /// The procedure's name token is inside this node.
    PROCEDURE_NAME,
    /// `[ a b ]` after the procedure name.
    PARAM_LIST,
    /// Everything between the header and `end`.
    BODY,
    /// `[ ... ]` inside a body.
    BRACKET_GROUP,
    /// `( ... )` inside a body.
    PAREN_GROUP,
    /// Tokens that could not be placed: top-level code outside a procedure, stray closers.
    ERROR,
}

impl SyntaxKind {
    pub fn is_trivia(self) -> bool {
        matches!(self, SyntaxKind::WHITESPACE | SyntaxKind::COMMENT)
    }
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(k: SyntaxKind) -> Self {
        rowan::SyntaxKind(k as u16)
    }
}

/// The rowan language for NetLogo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NetLogo {}

impl rowan::Language for NetLogo {
    type Kind = SyntaxKind;
    fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
        assert!(raw.0 <= SyntaxKind::ERROR as u16);
        // SAFETY: SyntaxKind is repr(u16) and the value is in range.
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }
    fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
        kind.into()
    }
}

pub type SyntaxNode = rowan::SyntaxNode<NetLogo>;
pub type SyntaxToken = rowan::SyntaxToken<NetLogo>;

/// A problem in the source, with the byte range it applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    pub span: Range<usize>,
    pub message: String,
}

/// The result of parsing: the tree and every error found.
#[derive(Debug, Clone)]
pub struct Parse {
    green: rowan::GreenNode,
    pub errors: Vec<SyntaxError>,
}

impl Parse {
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    pub fn source_file(&self) -> SourceFile {
        SourceFile::cast(self.syntax()).expect("root is SOURCE_FILE")
    }
}

/// Lexes and parses NetLogo code (a model's code section or an `.nls` file).
pub fn parse(source: &str) -> Parse {
    let (tokens, lex_errors) = lex(source);
    let (green, mut errors) = parser::parse(source, &tokens);
    errors.extend(lex_errors.into_iter().map(|e| SyntaxError {
        span: e.span,
        message: e.message.into(),
    }));
    errors.sort_by_key(|e| (e.span.start, e.span.end));
    Parse { green, errors }
}
