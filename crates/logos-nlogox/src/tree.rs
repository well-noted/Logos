//! The element tree of an `.nlogox` file, with the byte span of every part.

use std::ops::Range;

use quick_xml::events::{BytesStart, Event};

/// An element, with spans into the file's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub name: String,
    /// From `<` of the start tag to `>` of the end tag (or of `/>`).
    pub span: Range<usize>,
    /// The start tag, `<name ...>` or `<name .../>`.
    pub start_tag: Range<usize>,
    /// Everything between the start and end tags. `None` for an empty-element tag (`<a/>`).
    pub content: Option<Range<usize>>,
    pub attributes: Vec<Attribute>,
    pub children: Vec<Node>,
}

/// An attribute in a start tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub name: String,
    /// The name as written.
    pub name_span: Range<usize>,
    /// The value as written, between the quotes, with entities not expanded.
    pub value_span: Range<usize>,
    /// `b'"'` or `b'\''`.
    pub quote: u8,
}

/// A child of an element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    /// Character data, including entity references such as `&amp;`.
    Text(Range<usize>),
    /// A whole `<![CDATA[...]]>` section.
    CData(Range<usize>),
    Comment(Range<usize>),
    /// A processing instruction or anything else kept as written.
    Other(Range<usize>),
}

impl Element {
    /// Child elements, in order.
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            _ => None,
        })
    }

    /// The first child element with this name.
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.name == name)
    }

    pub fn attribute(&self, name: &str) -> Option<&Attribute> {
        self.attributes.iter().find(|a| a.name == name)
    }

    /// The element at a path of child-element indices, starting from this one.
    pub fn at(&self, path: &[usize]) -> Option<&Element> {
        let mut e = self;
        for &i in path {
            e = e.elements().nth(i)?;
        }
        Some(e)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("not well-formed XML at byte {position}: {message}")]
    Xml { position: usize, message: String },
    #[error("the file has no root element")]
    NoRoot,
    #[error("content after the root element at byte {0}")]
    AfterRoot(usize),
}

/// Parses `text` into the root element. `base` is added to every span (for a byte order mark
/// that was skipped).
pub(crate) fn parse(text: &str, base: usize) -> Result<Element, ParseError> {
    let mut reader = quick_xml::Reader::from_str(text);
    let config = reader.config_mut();
    config.trim_text(false);
    config.expand_empty_elements = false;
    config.check_end_names = true;
    config.check_comments = true;

    let input_start = text.as_ptr() as usize;
    let offset_of = |s: &[u8]| -> usize { s.as_ptr() as usize - input_start + base };

    // Open elements, innermost last.
    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;
    loop {
        let start = reader.buffer_position() as usize + base;
        let event = reader.read_event().map_err(|e| ParseError::Xml {
            position: reader.error_position() as usize + base,
            message: e.to_string(),
        })?;
        let end = reader.buffer_position() as usize + base;
        let span = start..end;
        let node = match event {
            Event::Start(e) => {
                stack.push(element(
                    &e,
                    span.clone(),
                    Some(end..end),
                    &offset_of,
                    text,
                    base,
                )?);
                continue;
            }
            Event::Empty(e) => {
                Node::Element(element(&e, span.clone(), None, &offset_of, text, base)?)
            }
            Event::End(_) => {
                let mut e = stack.pop().expect("quick-xml checks end names");
                e.span.end = end;
                if let Some(c) = &mut e.content {
                    c.end = start;
                }
                Node::Element(e)
            }
            Event::Text(_) | Event::GeneralRef(_) => Node::Text(span),
            Event::CData(_) => Node::CData(span),
            Event::Comment(_) => Node::Comment(span),
            Event::Decl(_) | Event::PI(_) | Event::DocType(_) => Node::Other(span),
            Event::Eof => break,
        };
        match stack.last_mut() {
            Some(parent) => push_merging_text(&mut parent.children, node),
            // Prolog and trailing whitespace, comments and declarations stay in the text; they
            // are outside the root element and not part of the tree.
            None => {
                if let Node::Element(e) = node {
                    if root.is_some() {
                        return Err(ParseError::AfterRoot(e.span.start));
                    }
                    root = Some(e);
                }
            }
        }
    }
    if let Some(open) = stack.last() {
        return Err(ParseError::Xml {
            position: open.span.start,
            message: format!("element <{}> is not closed", open.name),
        });
    }
    root.ok_or(ParseError::NoRoot)
}

/// quick-xml reports text and entity references as separate events; one run of character
/// data is one node here.
fn push_merging_text(children: &mut Vec<Node>, node: Node) {
    if let (Some(Node::Text(prev)), Node::Text(next)) = (children.last_mut(), &node)
        && prev.end == next.start
    {
        prev.end = next.end;
        return;
    }
    children.push(node);
}

fn element(
    e: &BytesStart<'_>,
    span: std::ops::Range<usize>,
    content: Option<Range<usize>>,
    offset_of: &dyn Fn(&[u8]) -> usize,
    text: &str,
    base: usize,
) -> Result<Element, ParseError> {
    let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
    let mut attributes = Vec::new();
    for a in e.attributes().with_checks(true) {
        let a = a.map_err(|err| ParseError::Xml {
            position: span.start,
            message: err.to_string(),
        })?;
        let key = a.key.as_ref();
        let value: &[u8] = &a.value;
        let in_input = |s: &[u8]| {
            let p = s.as_ptr() as usize;
            let lo = text.as_ptr() as usize;
            p >= lo && p + s.len() <= lo + text.len()
        };
        // The reader borrows from the input, so attribute slices point into it.
        if !in_input(key) || !(value.is_empty() || in_input(value)) {
            return Err(ParseError::Xml {
                position: span.start,
                message: "attribute not borrowed from the input".into(),
            });
        }
        let name_start = offset_of(key);
        let value_start = if value.is_empty() {
            // An empty value: find the opening quote after `=`.
            let after_name = name_start + key.len() - base;
            let rest = &text.as_bytes()[after_name..];
            let q = rest
                .iter()
                .position(|&b| b == b'"' || b == b'\'')
                .expect("quick-xml checked the attribute");
            after_name + q + 1 + base
        } else {
            offset_of(value)
        };
        let quote = text.as_bytes()[value_start - 1 - base];
        attributes.push(Attribute {
            name: String::from_utf8_lossy(key).into_owned(),
            name_span: name_start..name_start + key.len(),
            value_span: value_start..value_start + value.len(),
            quote,
        });
    }
    Ok(Element {
        name,
        start_tag: span.clone(),
        span,
        content,
        attributes,
        children: Vec::new(),
    })
}
