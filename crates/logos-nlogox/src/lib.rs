//! Reading and editing NetLogo `.nlogox` files without losing anything (Phase 0, Spike B).
//!
//! A [`Nlogox`] keeps the file's text exactly as read and an element tree with the byte span
//! of every element, attribute and text run. Writing an unmodified file returns the same
//! bytes. An edit replaces the bytes of the part it changes and nothing else, so attribute
//! order, whitespace, line endings, CDATA sections, comments and elements Logos doesn't know
//! stay as they were. A file is never re-serialized from a model of its content.
//!
//! New text is encoded the way NetLogo 7.0.4 writes it (see [`text`]).

pub mod text;
mod tree;

use std::ops::Range;

pub use text::DecodeError;
pub use tree::{Attribute, Element, Node, ParseError};

/// An `.nlogox` file.
#[derive(Debug, Clone)]
pub struct Nlogox {
    text: String,
    root: Element,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EditError {
    #[error("no element at {0:?}")]
    NoSuchElement(Vec<usize>),
    #[error("<{0}> has child elements; only text content can be replaced")]
    HasChildElements(String),
    #[error("no <{0}> section")]
    NoSection(String),
    #[error("the edit produced a file that does not parse: {0}")]
    Reparse(ParseError),
}

const BOM: &str = "\u{feff}";

impl Nlogox {
    pub fn parse(text: impl Into<String>) -> Result<Nlogox, ParseError> {
        let text = text.into();
        let root = parse_root(&text)?;
        Ok(Nlogox { text, root })
    }

    /// The file as it is now: the original bytes plus any edits.
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn into_text(self) -> String {
        self.text
    }

    /// The `<model>` element.
    pub fn root(&self) -> &Element {
        &self.root
    }

    /// The line end the file uses: `"\r\n"` if its first line ends that way, otherwise `"\n"`.
    pub fn line_end(&self) -> &'static str {
        match self.text.find('\n') {
            Some(i) if i > 0 && self.text.as_bytes()[i - 1] == b'\r' => "\r\n",
            _ => "\n",
        }
    }

    /// The text of `span`, exactly as written.
    pub fn raw(&self, span: &Range<usize>) -> &str {
        &self.text[span.clone()]
    }

    /// A top-level section such as `code`, `widgets` or `info`.
    pub fn section(&self, name: &str) -> Option<&Element> {
        self.root.child(name)
    }

    /// The text content of an element as NetLogo's reader returns it: entities expanded, CDATA
    /// unwrapped, line ends as `\n`. Content that is only whitespace reads as empty, as in
    /// NetLogo (`XMLReader` skips whitespace-only text).
    pub fn element_text(&self, element: &Element) -> Result<String, DecodeError> {
        let Some(content) = &element.content else {
            return Ok(String::new());
        };
        let s = text::decode_content(self.raw(content))?;
        Ok(if s.trim().is_empty() {
            String::new()
        } else {
            s
        })
    }

    /// The model's code (the `<code>` section).
    pub fn code(&self) -> Result<String, DecodeError> {
        self.section("code")
            .map_or(Ok(String::new()), |e| self.element_text(e))
    }

    /// The Info tab text (the `<info>` section).
    pub fn info(&self) -> Result<String, DecodeError> {
        self.section("info")
            .map_or(Ok(String::new()), |e| self.element_text(e))
    }

    /// The widgets in the `<widgets>` section, in file order.
    pub fn widgets(&self) -> Vec<&Element> {
        self.section("widgets")
            .map(|w| w.elements().collect())
            .unwrap_or_default()
    }

    /// The path (child-element indices from the root) of widget `index`.
    pub fn widget_path(&self, index: usize) -> Option<Vec<usize>> {
        let w = self.root.elements().position(|e| e.name == "widgets")?;
        (index < self.widgets().len()).then(|| vec![w, index])
    }

    /// The path of a top-level section.
    pub fn section_path(&self, name: &str) -> Option<Vec<usize>> {
        self.root
            .elements()
            .position(|e| e.name == name)
            .map(|i| vec![i])
    }

    /// An attribute's value as a reader returns it.
    pub fn attribute_value(&self, element: &Element, name: &str) -> Option<String> {
        element
            .attribute(name)
            .map(|a| text::decode_attribute(self.raw(&a.value_span)))
    }

    /// Replaces the code section's text.
    pub fn set_code(&mut self, code: &str) -> Result<(), EditError> {
        self.set_section_text("code", code)
    }

    /// Replaces the info section's text.
    pub fn set_info(&mut self, info: &str) -> Result<(), EditError> {
        self.set_section_text("info", info)
    }

    pub fn set_section_text(&mut self, section: &str, value: &str) -> Result<(), EditError> {
        let path = self
            .section_path(section)
            .ok_or_else(|| EditError::NoSection(section.into()))?;
        self.set_text(&path, value)
    }

    /// Replaces the content of the element at `path`, which must have no child elements. The
    /// start and end tags are kept as written.
    pub fn set_text(&mut self, path: &[usize], value: &str) -> Result<(), EditError> {
        let e = self
            .root
            .at(path)
            .ok_or_else(|| EditError::NoSuchElement(path.to_vec()))?;
        if e.elements().next().is_some() {
            return Err(EditError::HasChildElements(e.name.clone()));
        }
        let encoded = text::encode_content(value, self.line_end());
        let (range, replacement) = match &e.content {
            Some(content) => (content.clone(), encoded),
            None => {
                // `<name .../>` becomes `<name ...>value</name>`.
                let tag = self.raw(&e.start_tag);
                let open = tag.trim_end_matches('>').trim_end_matches('/').trim_end();
                (e.span.clone(), format!("{open}>{encoded}</{}>", e.name))
            }
        };
        self.replace(range, &replacement)
    }

    /// Sets an attribute of the element at `path`. An existing value is replaced in place,
    /// keeping its quote character; a new attribute is added at the end of the start tag.
    pub fn set_attribute(
        &mut self,
        path: &[usize],
        name: &str,
        value: &str,
    ) -> Result<(), EditError> {
        let e = self
            .root
            .at(path)
            .ok_or_else(|| EditError::NoSuchElement(path.to_vec()))?;
        match e.attribute(name) {
            Some(a) => {
                let encoded = text::encode_attribute(value, a.quote);
                self.replace(a.value_span.clone(), &encoded)
            }
            None => {
                let tag = self.raw(&e.start_tag);
                let close = if tag.ends_with("/>") { 2 } else { 1 };
                let mut at = e.start_tag.end - close;
                // Before any whitespace that precedes `>` or `/>`.
                while self.text.as_bytes()[at - 1].is_ascii_whitespace() {
                    at -= 1;
                }
                let encoded = text::encode_attribute(value, b'"');
                self.replace(at..at, &format!(" {name}=\"{encoded}\""))
            }
        }
    }

    fn replace(&mut self, range: Range<usize>, replacement: &str) -> Result<(), EditError> {
        let mut text = String::with_capacity(self.text.len() + replacement.len());
        text.push_str(&self.text[..range.start]);
        text.push_str(replacement);
        text.push_str(&self.text[range.end..]);
        let root = parse_root(&text).map_err(EditError::Reparse)?;
        self.text = text;
        self.root = root;
        Ok(())
    }
}

fn parse_root(text: &str) -> Result<Element, ParseError> {
    let base = if text.starts_with(BOM) { BOM.len() } else { 0 };
    tree::parse(&text[base..], base)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
<model version=\"NetLogo 7.0.4\" snapToGrid=\"false\">\n  \
<code><![CDATA[to go\n  if ticks < 5 [ tick ]\nend]]></code>\n  \
<widgets>\n    \
<slider x=\"5\" max=\"100\" display=\"rate\" variable='rate'></slider>\n    \
<button x=\"1\" kind=\"Observer\">go</button>\n    <!-- a comment -->\n    \
<future attr=\"kept\"/>\n  </widgets>\n  \
<info>A &lt;b&gt; test &amp; more</info>\n  <previewCommands/>\n</model>\n";

    #[test]
    fn reads_sections_and_spans() {
        let f = Nlogox::parse(SMALL).unwrap();
        assert_eq!(f.text(), SMALL);
        assert_eq!(f.root().name, "model");
        assert_eq!(f.code().unwrap(), "to go\n  if ticks < 5 [ tick ]\nend");
        assert_eq!(f.info().unwrap(), "A <b> test & more");
        let w = f.widgets();
        assert_eq!(
            w.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["slider", "button", "future"]
        );
        assert_eq!(f.attribute_value(w[0], "variable").as_deref(), Some("rate"));
        assert_eq!(w[0].attribute("variable").unwrap().quote, b'\'');
        assert_eq!(f.raw(&w[0].attribute("max").unwrap().value_span), "100");
        assert_eq!(f.element_text(w[1]).unwrap(), "go");
        assert_eq!(f.section("previewCommands").unwrap().content, None);
        assert_eq!(f.line_end(), "\n");
    }

    /// Asserts that `after` equals `before` with one range replaced, and returns the
    /// replaced range of `before`.
    fn changed_range(before: &str, after: &str) -> Range<usize> {
        let p = before
            .bytes()
            .zip(after.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        let s = before[p..]
            .bytes()
            .rev()
            .zip(after[p..].bytes().rev())
            .take_while(|(a, b)| a == b)
            .count();
        p..before.len() - s
    }

    #[test]
    fn edits_change_only_their_bytes() {
        let mut f = Nlogox::parse(SMALL).unwrap();
        let code_span = f.section("code").unwrap().content.clone().unwrap();
        let new_code = format!("{}\n; added", f.code().unwrap());
        f.set_code(&new_code).unwrap();
        let r = changed_range(SMALL, f.text());
        assert!(
            r.start >= code_span.start && r.end <= code_span.end,
            "{r:?}"
        );
        assert_eq!(f.code().unwrap(), new_code);

        let before = f.text().to_owned();
        let path = f.widget_path(0).unwrap();
        f.set_attribute(&path, "max", "200").unwrap();
        assert_eq!(changed_range(&before, f.text()), {
            let i = before.find("max=\"100\"").unwrap() + 5;
            i..i + 1
        });
        f.set_attribute(&path, "variable", "it's").unwrap();
        assert_eq!(
            f.attribute_value(f.widgets()[0], "variable").as_deref(),
            Some("it's")
        );
        assert!(f.text().contains("variable='it&apos;s'"));

        let path = f.widget_path(2).unwrap();
        f.set_attribute(&path, "new", "a\"b").unwrap();
        assert!(
            f.text()
                .contains("<future attr=\"kept\" new=\"a&quot;b\"/>")
        );

        f.set_section_text("previewCommands", "setup").unwrap();
        assert!(
            f.text()
                .contains("<previewCommands>setup</previewCommands>")
        );
        f.set_info("plain").unwrap();
        assert!(f.text().contains("<info>plain</info>"));
        assert!(f.text().contains("<!-- a comment -->"));
        assert!(matches!(
            f.set_section_text("widgets", "x"),
            Err(EditError::HasChildElements(_))
        ));
        assert!(matches!(
            f.set_section_text("nope", "x"),
            Err(EditError::NoSection(_))
        ));
    }

    #[test]
    fn keeps_crlf_and_bom() {
        let crlf = format!("{BOM}{}", SMALL.replace('\n', "\r\n"));
        let mut f = Nlogox::parse(crlf.clone()).unwrap();
        assert_eq!(f.text(), crlf);
        assert_eq!(f.line_end(), "\r\n");
        assert_eq!(f.code().unwrap(), "to go\n  if ticks < 5 [ tick ]\nend");
        f.set_code("to a\nend").unwrap();
        assert!(f.text().contains("<code>to a\r\nend</code>"));
        assert!(f.text().starts_with(BOM));
    }

    #[test]
    fn rejects_broken_xml() {
        assert!(Nlogox::parse("<model><code></model>").is_err());
        assert!(Nlogox::parse("").is_err());
        assert!(Nlogox::parse("<a/><b/>").is_err());
        assert!(Nlogox::parse("<model a=\"1\" a=\"2\"/>").is_err());
    }
}
