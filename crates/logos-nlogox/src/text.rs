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
    decode_content_mapped(raw, 0).map(|d| d.text)
}

/// Decoded element content and where each part of it is in the file, so that an editor can
/// convert between offsets in the decoded text (the model's code) and offsets in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub text: String,
    /// `(decoded offset, file offset)` at the start of each run of bytes that is the same in
    /// both, in order.
    anchors: Vec<(usize, usize)>,
}

impl Decoded {
    /// The file offset of a byte offset in the decoded text. An offset inside an expanded
    /// entity or a normalized line end maps to its start.
    pub fn to_file(&self, offset: usize) -> usize {
        let i = self.anchors.partition_point(|a| a.0 <= offset).max(1) - 1;
        let (d, f) = self.anchors[i];
        let mapped = f + offset.saturating_sub(d);
        match self.anchors.get(i + 1) {
            Some(&(_, next)) => mapped.min(next),
            None => mapped,
        }
    }

    /// Like [`Decoded::to_file`], for the end of a range: where text and markup meet, the
    /// offset maps to the end of the text before the markup.
    pub fn to_file_end(&self, offset: usize) -> usize {
        let i = self.anchors.partition_point(|a| a.0 < offset).max(1) - 1;
        let (d, f) = self.anchors[i];
        let mapped = f + offset.saturating_sub(d);
        match self.anchors.get(i + 1) {
            Some(&(_, next)) => mapped.min(next),
            None => mapped,
        }
    }

    /// The decoded offset of a file offset. A file offset inside markup (a CDATA delimiter,
    /// a comment) maps to the decoded offset where that markup is; one inside an entity, to
    /// the end of its expansion.
    pub fn from_file(&self, offset: usize) -> usize {
        let i = self.anchors.partition_point(|a| a.1 <= offset).max(1) - 1;
        let (d, f) = self.anchors[i];
        let mapped = d + offset.saturating_sub(f);
        let limit = self
            .anchors
            .get(i + 1)
            .map_or(self.text.len(), |&(next, _)| next);
        mapped.min(limit)
    }
}

struct Builder {
    text: String,
    anchors: Vec<(usize, usize)>,
}

impl Builder {
    fn anchor(&mut self, file: usize) {
        let here = self.text.len();
        if let Some(last) = self.anchors.last_mut()
            && last.0 == here
        {
            *last = (here, file);
            return;
        }
        if let Some(&(d, f)) = self.anchors.last()
            && f + (here - d) == file
        {
            return;
        }
        self.anchors.push((here, file));
    }

    /// Literal text that starts at file offset `at`, with line ends normalized and, in a
    /// CDATA section, NetLogo's escape for `]]>` undone.
    fn literal(&mut self, at: usize, s: &str, cdata: bool) {
        const ESCAPED: &str = "]]\u{E000}>";
        self.anchor(at);
        let mut i = 0;
        while i < s.len() {
            let rest = &s[i..];
            if rest.starts_with("\r\n") {
                self.anchor(at + i);
                self.text.push('\n');
                i += 2;
                self.anchor(at + i);
            } else if rest.starts_with('\r') {
                self.text.push('\n');
                i += 1;
            } else if cdata && rest.starts_with(ESCAPED) {
                self.text.push_str("]]");
                i += 2 + CDATA_ESCAPE.len_utf8();
                self.anchor(at + i);
            } else {
                let c = rest.chars().next().unwrap_or_default();
                self.text.push(c);
                i += c.len_utf8();
            }
        }
    }
}

/// [`decode_content`], with a map back to the file; `base` is the file offset of `raw`.
pub fn decode_content_mapped(raw: &str, base: usize) -> Result<Decoded, DecodeError> {
    let mut b = Builder {
        text: String::with_capacity(raw.len()),
        anchors: Vec::new(),
    };
    b.anchor(base);
    let mut i = 0;
    while i < raw.len() {
        let rest = &raw[i..];
        if let Some(body) = rest.strip_prefix("<![CDATA[") {
            let end = body.find("]]>").ok_or(DecodeError::Unterminated {
                what: "CDATA section",
                at: i,
            })?;
            let start = i + "<![CDATA[".len();
            b.literal(base + start, &body[..end], true);
            i = start + end + "]]>".len();
            b.anchor(base + i);
        } else if let Some(body) = rest.strip_prefix("<!--") {
            let end = body.find("-->").ok_or(DecodeError::Unterminated {
                what: "comment",
                at: i,
            })?;
            i += "<!--".len() + end + "-->".len();
            b.anchor(base + i);
        } else if rest.starts_with('<') {
            return Err(DecodeError::Markup(i));
        } else if rest.starts_with('&') {
            let (s, used) = entity(rest);
            b.anchor(base + i);
            b.text.push_str(&s);
            i += used;
            b.anchor(base + i);
        } else {
            let next = rest.find(['<', '&']).unwrap_or(rest.len());
            b.literal(base + i, &rest[..next], false);
            i += next;
        }
    }
    Ok(Decoded {
        text: b.text,
        anchors: b.anchors,
    })
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
    fn maps_decoded_offsets_to_the_file() {
        let raw = "x<!-- c -->y\r\nz&lt;w<![CDATA[a\r\nb]]\u{E000}>c]]>d";
        let d = decode_content_mapped(raw, 100).unwrap();
        assert_eq!(d.text, decode_content(raw).unwrap());
        assert_eq!(d.text, "xy\nz<wa\nb]]>cd");
        // Every character of the decoded text that is written as itself is found there.
        for (i, c) in d.text.char_indices() {
            let f = d.to_file(i) - 100;
            if !matches!(c, '\n' | '<' | '>') {
                assert_eq!(raw[f..].chars().next(), Some(c), "at {i}");
            }
            assert_eq!(d.from_file(d.to_file(i)), i, "at {i}");
        }
        assert_eq!(d.to_file(d.text.len()), 100 + raw.len());
        let cdata = "<![CDATA[to go end]]>";
        let c = decode_content_mapped(cdata, 0).unwrap();
        assert_eq!(c.to_file(0), 9);
        assert_eq!(c.to_file_end(c.text.len()), cdata.len() - 3);
        // Inside markup, a file offset maps to the end of the text before it, or of the
        // entity's expansion.
        assert_eq!(
            d.from_file(100 + raw.find("&lt;").unwrap() + 2),
            d.text.find('<').unwrap() + 1
        );
        assert_eq!(d.from_file(100 + 3), 1);
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
