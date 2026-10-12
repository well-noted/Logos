//! Problems to show in an editor. Compile errors come from NetLogo (D-07); Logos's own parser
//! adds the syntax errors it finds elsewhere in the code, since NetLogo stops at its first
//! error.

use std::ops::Range;

use logos_protocol::ModelError;
use logos_syntax::SyntaxError;

use crate::text::{byte_range, utf16_to_byte};

/// Who found a problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// NetLogo's compiler, through the bridge.
    NetLogo,
    /// Logos's parser.
    Logos,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Byte range in the source, or in `file` when it is set.
    pub range: Range<usize>,
    pub message: String,
    pub origin: Origin,
    /// The include file the problem is in, as NetLogo names it; `None` for the source itself.
    pub file: Option<String>,
}

/// Combines NetLogo's compile errors for `source` (from `workspace.check`, UTF-16 offsets)
/// with the parser's syntax errors (byte offsets). A parser error is left out where it
/// overlaps a NetLogo error, so each problem is shown once, in NetLogo's words.
///
/// Errors in include files keep NetLogo's offsets in `range`; the caller maps them with the
/// include file's text.
pub fn diagnostics(
    source: &str,
    netlogo: &[ModelError],
    parser: &[SyntaxError],
) -> Vec<Diagnostic> {
    let map = utf16_to_byte(source);
    let mut out: Vec<Diagnostic> = netlogo
        .iter()
        .map(|e| {
            let (s, t) = (e.span.start as usize, e.span.end as usize);
            Diagnostic {
                range: if e.span.file.is_none() {
                    byte_range(&map, s, t)
                } else {
                    s..t
                },
                message: e.message.clone(),
                origin: Origin::NetLogo,
                file: e.span.file.clone(),
            }
        })
        .collect();
    for p in parser {
        let overlaps = out.iter().any(|d| {
            d.file.is_none() && d.range.start <= p.span.end && p.span.start <= d.range.end
        });
        if !overlaps {
            out.push(Diagnostic {
                range: p.span.clone(),
                message: p.message.clone(),
                origin: Origin::Logos,
                file: None,
            });
        }
    }
    out.sort_by_key(|d| (d.file.clone(), d.range.start));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use logos_protocol::SourceSpan;

    #[test]
    fn netlogo_errors_come_first_and_are_not_repeated() {
        let source = "; caf\u{e9}\nto go fd foo end\nto b ]\n";
        let start16 = source[..source.find("foo").unwrap()].encode_utf16().count() as u64;
        let netlogo = vec![ModelError {
            message: "Nothing named FOO has been defined.".into(),
            span: SourceSpan {
                start: start16,
                end: start16 + 3,
                file: None,
            },
        }];
        let foo = source.find("foo").unwrap();
        let close = source.find(']').unwrap();
        let parser = vec![
            SyntaxError {
                span: foo..foo + 3,
                message: "Nothing named FOO has been defined.".into(),
            },
            SyntaxError {
                span: close..close + 1,
                message: "] without a matching opening bracket".into(),
            },
        ];
        let d = diagnostics(source, &netlogo, &parser);
        assert_eq!(d.len(), 2);
        assert_eq!(&source[d[0].range.clone()], "foo");
        assert_eq!(d[0].origin, Origin::NetLogo);
        assert_eq!(&source[d[1].range.clone()], "]");
        assert_eq!(d[1].origin, Origin::Logos);
    }
}
