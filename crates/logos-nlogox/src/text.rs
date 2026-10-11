//! Decoding text and attribute values as an XML reader sees them, and encoding new values the
//! way NetLogo 7.0.4 writes them (`api/XMLUtils.scala`, `XMLWriter`).

/// NetLogo writes `]]>` inside a CDATA section as `]]`, U+E000, `>` and its reader turns that
/// back into `]]>` (`core/model/XMLElement.scala`, `CDataEscape`).
pub const CDATA_ESCAPE: char = '\u{E000}';

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DecodeError {
    #[error("unterminated {what} at byte {at}")]
    Unterminated { what: &'static str, at: usize },
    #[error("element content at byte {0}; expected text")]
    Markup(usize),
}

/// The character data of element content written as `raw`: text with entity references
/// expanded and CDATA sections unwrapped, comments dropped, and line ends normalized to `\n`
/// as an XML reader does. An entity NetLogo's reader doesn't know (anything other than the five
/// predefined ones and character references) is kept as written.
pub fn decode_content(raw: &str) -> Result<String, DecodeError> {
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let rest = &raw[i..];
        if let Some(body) = rest.strip_prefix("<![CDATA[") {
            let end = body.find("]]>").ok_or(DecodeError::Unterminated {
                what: "CDATA section",
                at: i,
            })?;
            let inner = normalize_newlines(&body[..end]);
            out.push_str(&inner.replace(&format!("]]{CDATA_ESCAPE}>"), "]]>"));
            i += "<![CDATA[".len() + end + "]]>".len();
        } else if let Some(body) = rest.strip_prefix("<!--") {
            let end = body.find("-->").ok_or(DecodeError::Unterminated {
                what: "comment",
                at: i,
            })?;
            i += "<!--".len() + end + "-->".len();
        } else if rest.starts_with('<') {
            return Err(DecodeError::Markup(i));
        } else if rest.starts_with('&') {
            let (s, used) = entity(rest);
            out.push_str(&s);
            i += used;
        } else {
            let next = rest.find(['<', '&']).unwrap_or(rest.len());
            out.push_str(&normalize_newlines(&rest[..next]));
            i += next;
        }
    }
    Ok(out)
}

/// An attribute value written as `raw` (between the quotes), as an XML reader returns it:
/// entities expanded and literal tabs and line ends replaced by spaces.
pub fn decode_attribute(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let rest = &raw[i..];
        if rest.starts_with('&') {
            let (s, used) = entity(rest);
            out.push_str(&s);
            i += used;
        } else {
            let next = rest.find('&').unwrap_or(rest.len());
            let literal = normalize_newlines(&rest[..next]);
            out.extend(literal.chars().map(|c| match c {
                '\t' | '\n' => ' ',
                c => c,
            }));
            i += next;
        }
    }
    out
}

/// Expands one entity reference at the start of `s`. Returns the text and the bytes used.
fn entity(s: &str) -> (String, usize) {
    let Some(end) = s.find(';').filter(|&e| e <= 12) else {
        return ("&".into(), 1);
    };
    let name = &s[1..end];
    let expanded = match name {
        "lt" => Some('<'),
        "gt" => Some('>'),
        "amp" => Some('&'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let code = if let Some(hex) = name.strip_prefix("#x") {
                u32::from_str_radix(hex, 16).ok()
            } else if let Some(dec) = name.strip_prefix('#') {
                dec.parse().ok()
            } else {
                None
            };
            code.and_then(char::from_u32)
        }
    };
    match expanded {
        Some(c) => (c.to_string(), end + 1),
        None => (s[..=end].to_owned(), end + 1),
    }
}

fn normalize_newlines(s: &str) -> std::borrow::Cow<'_, str> {
    if s.contains('\r') {
        s.replace("\r\n", "\n").replace('\r', "\n").into()
    } else {
        s.into()
    }
}

/// Element content for `text`, as NetLogo writes it: a CDATA section when the text contains
/// `<`, `>` or `&`, plain characters otherwise. Line ends are written as `line_end`.
pub fn encode_content(text: &str, line_end: &str) -> String {
    let text = normalize_newlines(text);
    let body = if text.contains(['<', '>', '&']) {
        format!(
            "<![CDATA[{}]]>",
            text.replace("]]>", &format!("]]{CDATA_ESCAPE}>"))
        )
    } else {
        text.into_owned()
    };
    if line_end == "\n" {
        body
    } else {
        body.replace('\n', line_end)
    }
}

/// An attribute value for `value`, to go between `quote` characters. Escapes `&`, `<`, `>`
/// and `"` as NetLogo's writer does, the quote character in use, and tabs and line ends, which
/// a reader would otherwise turn into spaces.
pub fn encode_attribute(value: &str, quote: u8) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' if quote == b'\'' => out.push_str("&apos;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_text_cdata_entities_and_newlines() {
        assert_eq!(
            decode_content("a &lt; b &amp;&#65;&#x42;").unwrap(),
            "a < b &AB"
        );
        assert_eq!(
            decode_content("<![CDATA[if x < 1 [ ]]>").unwrap(),
            "if x < 1 [ "
        );
        assert_eq!(decode_content("x<!-- c -->y\r\nz\rw").unwrap(), "xy\nz\nw");
        assert_eq!(
            decode_content("<![CDATA[a]]\u{E000}>b]]>").unwrap(),
            "a]]>b"
        );
        assert_eq!(decode_content("&harr; &").unwrap(), "&harr; &");
        assert!(decode_content("<![CDATA[open").is_err());
        assert_eq!(decode_content("a<b/>"), Err(DecodeError::Markup(1)));
    }

    #[test]
    fn decodes_attributes() {
        assert_eq!(decode_attribute("a&quot;b\tc\r\nd&#10;"), "a\"b c d\n");
    }

    #[test]
    fn encodes_like_netlogo() {
        assert_eq!(encode_content("to go\nend", "\n"), "to go\nend");
        assert_eq!(
            encode_content("if a < b [\n]", "\r\n"),
            "<![CDATA[if a < b [\r\n]]]>"
        );
        assert_eq!(
            encode_content("x ]]> y &", "\n"),
            "<![CDATA[x ]]\u{E000}> y &]]>"
        );
        for s in ["a < b", "x ]]> y", "plain", "a\nb"] {
            assert_eq!(decode_content(&encode_content(s, "\r\n")).unwrap(), s);
        }
        assert_eq!(
            encode_attribute("a\"<&>'\n", b'"'),
            "a&quot;&lt;&amp;&gt;'&#10;"
        );
        assert_eq!(encode_attribute("it's", b'\''), "it&apos;s");
        assert_eq!(
            decode_attribute(&encode_attribute("a\tb\nc", b'"')),
            "a\tb\nc"
        );
    }
}
