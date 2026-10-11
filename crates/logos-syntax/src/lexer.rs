//! The lexer. It follows NetLogo 7.0.4's `TokenLexer` (`parser-core/src/main/lex`) and
//! `Charset` (`parser-jvm/src/main/lex`), with one difference: every byte of the input belongs
//! to exactly one token, including whitespace, so joining the tokens reproduces the source.

use std::ops::Range;

use crate::SyntaxKind;

/// A token: its kind and its byte range in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: SyntaxKind,
    pub span: Range<usize>,
}

/// A problem the lexer found. The token is still produced (as an `ERROR_TOKEN` or with its
/// usual kind) so the source stays covered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub span: Range<usize>,
    pub message: &'static str,
}

/// Punctuation NetLogo allows in identifiers, besides letters and digits.
const IDENT_PUNCTUATION: &str = "_.?=*!<>:#+/%$^'&-";

/// NetLogo's `Charset.validIdentifierChar`: a digit, one of ``_.?=*!<>:#+/%$^'&-``, or a letter.
///
/// NetLogo uses Java's `Character.isLetter` on UTF-16 code units, so a character outside the
/// Basic Multilingual Plane (a surrogate pair) is never an identifier character. For letters in
/// the BMP this uses Rust's `is_alphabetic`, which also accepts letter-like numbers and
/// combining vowel signs that Java's `isLetter` rejects; models using those are not expected.
pub fn is_identifier_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || IDENT_PUNCTUATION.contains(c)
        || ((c as u32) < 0x10000 && !c.is_ascii() && c.is_alphabetic())
}

/// Java's `Character.isWhitespace`, which NetLogo uses between tokens. Unlike Rust's
/// `char::is_whitespace` it excludes no-break spaces and includes U+001C..U+001F.
pub fn is_whitespace(c: char) -> bool {
    matches!(c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | '\u{1C}'..='\u{1F}' | ' '
        | '\u{1680}' | '\u{2000}'..='\u{2006}' | '\u{2008}'..='\u{200A}'
        | '\u{2028}' | '\u{2029}' | '\u{205F}' | '\u{3000}')
}

/// Splits `source` into tokens that cover it exactly.
pub fn lex(source: &str) -> (Vec<Token>, Vec<LexError>) {
    let mut tokens = Vec::new();
    let mut errors = Vec::new();
    let mut i = 0;
    while i < source.len() {
        let rest = &source[i..];
        let (kind, len) = next_token(rest, i, &mut errors);
        debug_assert!(len > 0 && rest.is_char_boundary(len));
        tokens.push(Token {
            kind,
            span: i..i + len,
        });
        i += len;
    }
    (tokens, errors)
}

fn next_token(rest: &str, at: usize, errors: &mut Vec<LexError>) -> (SyntaxKind, usize) {
    use SyntaxKind::*;
    let first = rest.chars().next().unwrap();
    if is_whitespace(first) {
        return (WHITESPACE, prefix_len(rest, is_whitespace));
    }
    if rest.starts_with("{{") {
        return extension_literal(rest, at, errors);
    }
    let punct = match first {
        ',' => Some(COMMA),
        '{' => Some(L_BRACE),
        '}' => Some(R_BRACE),
        '(' => Some(L_PAREN),
        ')' => Some(R_PAREN),
        '[' => Some(L_BRACKET),
        ']' => Some(R_BRACKET),
        _ => None,
    };
    if let Some(k) = punct {
        return (k, 1);
    }
    if first == ';' {
        return (COMMENT, rest.find(['\r', '\n']).unwrap_or(rest.len()));
    }
    if starts_number(rest) {
        let len = prefix_len(rest, is_identifier_char);
        if !valid_number(&rest[..len]) {
            errors.push(LexError {
                span: at..at + len,
                message: "not a valid number",
            });
        }
        return (NUMBER, len);
    }
    if first == '"' {
        return string(rest, at, errors);
    }
    if is_identifier_char(first) {
        return (IDENT, prefix_len(rest, is_identifier_char));
    }
    let len = first.len_utf8();
    errors.push(LexError {
        span: at..at + len,
        message: "This non-standard character is not allowed.",
    });
    (ERROR_TOKEN, len)
}

fn prefix_len(s: &str, f: impl Fn(char) -> bool) -> usize {
    s.char_indices()
        .find(|&(_, c)| !f(c))
        .map_or(s.len(), |(i, _)| i)
}

/// NetLogo's numeric-literal prefix: a digit, or `.` or `-` followed by a digit or by `.` and
/// a digit.
fn starts_number(s: &str) -> bool {
    let b = s.as_bytes();
    let digit = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit);
    match b[0] {
        b'0'..=b'9' => true,
        b'.' | b'-' => digit(1) || (b.get(1) == Some(&b'.') && digit(2)),
        _ => false,
    }
}

/// Whether NetLogo's `NumberParser` accepts the literal. It parses with Java's
/// `Double.parseDouble` and rejects values that overflow to infinity.
fn valid_number(s: &str) -> bool {
    // Java accepts a trailing `d`, `D`, `f` or `F` type suffix, and hex floats; NetLogo models
    // don't use them, and Rust's parser rejects them, so they count as invalid here.
    s.parse::<f64>().is_ok_and(f64::is_finite)
        && !s.eq_ignore_ascii_case("inf")
        && !s.eq_ignore_ascii_case("nan")
}

/// A string literal: `"` up to the next unescaped `"`. A string that reaches the end of the
/// line is unterminated; NetLogo reports "Closing double quote is missing".
fn string(rest: &str, at: usize, errors: &mut Vec<LexError>) -> (SyntaxKind, usize) {
    let bytes = rest.as_bytes();
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if i + 1 < bytes.len() && bytes[i + 1] != b'\n' && bytes[i + 1] != b'\r' => {
                // Skip the escaped character (it may be multi-byte).
                i += 1 + rest[i + 1..].chars().next().unwrap().len_utf8();
                continue;
            }
            b'"' => return (SyntaxKind::STRING, i + 1),
            b'\n' | b'\r' => break,
            _ => i += 1,
        }
    }
    errors.push(LexError {
        span: at..at + i,
        message: "Closing double quote is missing",
    });
    (SyntaxKind::STRING, i)
}

/// An extension literal `{{ ... }}`, with nested `{{ }}` pairs, on one line.
fn extension_literal(rest: &str, at: usize, errors: &mut Vec<LexError>) -> (SyntaxKind, usize) {
    let b = rest.as_bytes();
    let mut depth = 0usize;
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\n' || b[i] == b'\r' {
            break;
        }
        if b[i..].starts_with(b"{{") {
            depth += 1;
            i += 2;
        } else if b[i..].starts_with(b"}}") {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return (SyntaxKind::EXTENSION_LITERAL, i);
            }
        } else {
            i += rest[i..].chars().next().unwrap().len_utf8();
        }
    }
    errors.push(LexError {
        span: at..at + i,
        message: "End of line reached unexpectedly",
    });
    (SyntaxKind::EXTENSION_LITERAL, i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use SyntaxKind::*;

    fn kinds(s: &str) -> Vec<(SyntaxKind, &str)> {
        let (t, _) = lex(s);
        t.iter().map(|t| (t.kind, &s[t.span.clone()])).collect()
    }

    #[test]
    fn covers_every_byte() {
        let src = "to go ; comment\r\n  ask turtles [ fd 1.5e3 set label \"a \\\"b\\\"\" ]\nend";
        let (t, e) = lex(src);
        assert!(e.is_empty(), "{e:?}");
        let joined: String = t.iter().map(|t| &src[t.span.clone()]).collect();
        assert_eq!(joined, src);
    }

    #[test]
    fn identifiers_numbers_and_punctuation() {
        assert_eq!(
            kinds("set-x! -5 x-1 .5 -.5 a?b [1,2]"),
            [
                (IDENT, "set-x!"),
                (WHITESPACE, " "),
                (NUMBER, "-5"),
                (WHITESPACE, " "),
                (IDENT, "x-1"),
                (WHITESPACE, " "),
                (NUMBER, ".5"),
                (WHITESPACE, " "),
                (NUMBER, "-.5"),
                (WHITESPACE, " "),
                (IDENT, "a?b"),
                (WHITESPACE, " "),
                (L_BRACKET, "["),
                (NUMBER, "1"),
                (COMMA, ","),
                (NUMBER, "2"),
                (R_BRACKET, "]"),
            ]
        );
        // `-` alone, `-x` and `'` are identifiers; `5-3` is one (invalid) number token.
        assert_eq!(
            kinds("- -x '"),
            [
                (IDENT, "-"),
                (WHITESPACE, " "),
                (IDENT, "-x"),
                (WHITESPACE, " "),
                (IDENT, "'")
            ]
        );
        let (t, e) = lex("5-3");
        assert_eq!(t.len(), 1);
        assert_eq!(e[0].message, "not a valid number");
        assert_eq!(kinds("über"), [(IDENT, "über")]);
    }

    #[test]
    fn strings_comments_and_literals() {
        assert_eq!(
            kinds("\"a\\\\\" x"),
            [(STRING, "\"a\\\\\""), (WHITESPACE, " "), (IDENT, "x")]
        );
        let (t, e) = lex("\"open\nnext");
        assert_eq!((t[0].kind, t[0].span.clone()), (STRING, 0..5));
        assert_eq!(e[0].message, "Closing double quote is missing");
        assert_eq!(
            kinds(";c\r\nx"),
            [(COMMENT, ";c"), (WHITESPACE, "\r\n"), (IDENT, "x")]
        );
        assert_eq!(
            kinds("{{array: 1 {{x}} }}{"),
            [(EXTENSION_LITERAL, "{{array: 1 {{x}} }}"), (L_BRACE, "{")]
        );
        let (_, e) = lex("{{open\n");
        assert_eq!(e[0].message, "End of line reached unexpectedly");
        let (t, e) = lex("a\u{00A0}b");
        assert_eq!(
            t[1].kind, ERROR_TOKEN,
            "no-break space is not whitespace in Java"
        );
        assert_eq!(e.len(), 1);
        assert_eq!(kinds("\u{1D465}")[0].0, ERROR_TOKEN);
    }
}
