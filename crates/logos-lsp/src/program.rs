//! Documents and the programs they belong to. A model (`.nlogox`) is a program with its code
//! and the `.nls` files its `__includes` names; an `.nls` file opened on its own belongs to the
//! model in its folder that includes it, or else is a program by itself.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use logos_nlogox::{Decoded, Nlogox};
use logos_syntax::Language;

use crate::analysis::{Analysis, Key, Source};
use crate::text::LineIndex;

/// The code in a document and how offsets in it relate to the document's text.
#[derive(Debug, Clone)]
pub struct CodeView {
    pub code: String,
    /// For a model: the code section's place in the file. `None` for an `.nls` file, whose
    /// text is the code.
    map: Option<Decoded>,
    /// Why a model's code couldn't be read (the file isn't well-formed XML, for example).
    pub error: Option<String>,
}

impl CodeView {
    pub fn of(text: &str, is_model: bool) -> CodeView {
        if !is_model {
            return CodeView {
                code: text.to_owned(),
                map: None,
                error: None,
            };
        }
        let failed = |e: String| CodeView {
            code: String::new(),
            map: None,
            error: Some(e),
        };
        match Nlogox::parse(text) {
            Err(e) => failed(format!("the model file can't be read: {e}")),
            Ok(f) => match f.code_mapped() {
                None => CodeView {
                    code: String::new(),
                    map: None,
                    error: None,
                },
                Some(Err(e)) => failed(format!("the code section can't be read: {e}")),
                Some(Ok(d)) => CodeView {
                    code: d.text.clone(),
                    map: Some(d),
                    error: None,
                },
            },
        }
    }

    /// The document offset of a code offset.
    pub fn to_doc(&self, offset: usize) -> usize {
        self.map.as_ref().map_or(offset, |m| m.to_file(offset))
    }

    /// The document range of a code range.
    pub fn range_to_doc(&self, r: &Range<usize>) -> Range<usize> {
        let end = self.map.as_ref().map_or(r.end, |m| m.to_file_end(r.end));
        self.to_doc(r.start).min(end)..end
    }

    /// The code offset of a document offset.
    pub fn from_doc(&self, offset: usize) -> usize {
        self.map
            .as_ref()
            .map_or(offset, |m| m.from_file(offset))
            .min(self.code.len())
    }
}

/// How the server names a file: its path, or the URI for a document that isn't a file.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FileKey {
    Path(PathBuf),
    Other(String),
}

impl FileKey {
    pub fn path(&self) -> Option<&Path> {
        match self {
            FileKey::Path(p) => Some(p),
            FileKey::Other(_) => None,
        }
    }

    pub fn is_model(&self) -> bool {
        let name = match self {
            FileKey::Path(p) => p.to_string_lossy().into_owned(),
            FileKey::Other(u) => u.clone(),
        };
        name.to_ascii_lowercase().ends_with(".nlogox")
    }
}

/// One source of a program, with the document it is in.
#[derive(Debug, Clone)]
pub struct Member {
    pub key: FileKey,
    /// The document's text (the whole `.nlogox` for a model).
    pub text: String,
    pub lines: LineIndex,
    pub view: CodeView,
}

impl Member {
    pub fn new(key: FileKey, text: String) -> Member {
        let view = CodeView::of(&text, key.is_model());
        Member {
            lines: LineIndex::new(&text),
            key,
            text,
            view,
        }
    }

    /// LSP (line, UTF-16 column) of a code offset.
    pub fn position(&self, code_offset: usize) -> (u32, u32) {
        self.lines
            .position(&self.text, self.view.to_doc(code_offset))
    }

    /// LSP positions of a code range.
    pub fn range(&self, r: &Range<usize>) -> ((u32, u32), (u32, u32)) {
        let d = self.view.range_to_doc(r);
        (
            self.lines.position(&self.text, d.start),
            self.lines.position(&self.text, d.end),
        )
    }

    /// The code offset of an LSP position.
    pub fn offset(&self, line: u32, col: u32) -> usize {
        self.view.from_doc(self.lines.offset(&self.text, line, col))
    }
}

/// A program: its root document (a model, or an `.nls` file on its own), its include files
/// and the analysis of all of them.
pub struct Program {
    pub root: FileKey,
    /// The root first, then include files in the order they were found.
    pub members: Vec<Member>,
    pub interface_globals: Vec<String>,
    pub extensions: Vec<String>,
    pub analysis: Analysis,
}

impl Program {
    /// Builds the program rooted at `root`. `read` returns a file's text: the editor's copy
    /// if the file is open, otherwise the file on disk.
    pub fn build(
        root: FileKey,
        read: &dyn Fn(&FileKey) -> Option<String>,
        language: &dyn Fn(&[String]) -> Option<Arc<Language>>,
    ) -> Program {
        let root_text = read(&root).unwrap_or_default();
        let first = Member::new(root.clone(), root_text);
        let interface_globals = if root.is_model() {
            interface_globals(&first.text)
        } else {
            Vec::new()
        };
        let mut members = vec![first];
        // Follow `__includes` (a model's only; include files can't include others in NetLogo).
        if root.is_model()
            && let Some(dir) = root.path().and_then(Path::parent)
        {
            let structure = logos_syntax::structure_of([members[0].view.code.as_str()]);
            for inc in &structure.includes {
                let key = FileKey::Path(normalize(&dir.join(inc)));
                if members.iter().any(|m| m.key == key) {
                    continue;
                }
                if let Some(text) = read(&key) {
                    members.push(Member::new(key, text));
                }
            }
        }
        let structure = logos_syntax::structure_of(members.iter().map(|m| m.view.code.as_str()));
        let mut extensions: Vec<String> = structure
            .extensions
            .iter()
            .map(|e| e.to_ascii_lowercase())
            .collect();
        extensions.sort();
        extensions.dedup();
        let lang = language(&extensions);
        let sources = members
            .iter()
            .map(|m| Source {
                name: format!("{:?}", m.key),
                code: m.view.code.clone(),
            })
            .collect();
        let analysis = Analysis::new(sources, interface_globals.clone(), lang);
        Program {
            root,
            members,
            interface_globals,
            extensions,
            analysis,
        }
    }

    pub fn member(&self, key: &FileKey) -> Option<usize> {
        self.members.iter().position(|m| m.key == *key)
    }

    /// Where `key` is used in the model file outside the code section: in buttons, monitors,
    /// plots and pens, BehaviorSpace experiments (including the variables they vary) and the
    /// preview commands. Byte ranges in the model file's text. Empty for an `.nls` program.
    pub fn uses_outside_code(&self, key: &Key) -> Vec<Range<usize>> {
        if !self.root.is_model() {
            return Vec::new();
        }
        let Ok(f) = Nlogox::parse(self.members[0].text.as_str()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for section in ["widgets", "experiments", "previewCommands"] {
            if let Some(e) = f.section(section) {
                self.visit(&f, e, key, &mut out);
            }
        }
        out.sort_by_key(|r| r.start);
        out
    }

    fn visit(&self, f: &Nlogox, e: &logos_nlogox::Element, key: &Key, out: &mut Vec<Range<usize>>) {
        // Elements whose text is NetLogo code, and whether it is a reporter.
        let reporter = match e.name.as_str() {
            "button" | "setup" | "update" | "go" | "preExperiment" | "postExperiment"
            | "preRun" | "postRun" | "previewCommands" => Some(false),
            "monitor" | "metric" | "exitCondition" | "runMetricsCondition" => Some(true),
            _ => None,
        };
        if let (Some(reporter), Some(content)) = (reporter, &e.content)
            && let Ok(d) = logos_nlogox::text::decode_content_mapped(f.raw(content), content.start)
        {
            for r in self.analysis.uses_in_snippet(&d.text, reporter, key) {
                out.push(d.to_file(r.start)..d.to_file_end(r.end));
            }
        }
        // A global varied by an experiment.
        if let Key::Global(name) = key
            && matches!(e.name.as_str(), "enumeratedValueSet" | "steppedValueSet")
            && let Some(a) = e.attribute("variable")
            && f.attribute_value(e, "variable")
                .is_some_and(|v| v.eq_ignore_ascii_case(name))
        {
            out.push(a.value_span.clone());
        }
        for child in e.elements() {
            self.visit(f, child, key, out);
        }
    }
}

/// The globals a model's interface widgets define (upper case).
pub fn interface_globals(model_text: &str) -> Vec<String> {
    let Ok(f) = Nlogox::parse(model_text) else {
        return Vec::new();
    };
    f.widgets()
        .into_iter()
        .filter_map(|w| f.attribute_value(w, "variable"))
        .filter(|v| !v.is_empty())
        .map(|v| v.to_ascii_uppercase())
        .collect()
}

/// A path without `.` and `..` parts, for comparing paths that name the same file.
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

/// The `.nlogox` files in `dir` whose code includes `nls` (by file name, as NetLogo resolves
/// `__includes` relative to the model's folder).
pub fn models_including(dir: &Path, nls: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if !p
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("nlogox"))
        {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        let Ok(f) = Nlogox::parse(text) else {
            continue;
        };
        let Ok(code) = f.code() else { continue };
        let s = logos_syntax::structure_of([code.as_str()]);
        if s.includes
            .iter()
            .any(|i| normalize(&dir.join(i)) == normalize(nls))
        {
            out.push(p);
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_model_code_positions() {
        let code = "globals [g]\r\nto go\r\n  set g 1 ; caf\u{e9} &\r\nend";
        let text = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<model>\r\n  <code><![CDATA[{code}]]></code>\r\n</model>\r\n"
        );
        let m = Member::new(FileKey::Other("untitled:m.nlogox".into()), text.clone());
        assert_eq!(m.view.code, code.replace("\r\n", "\n"));
        let at = m.view.code.find("set").unwrap();
        let (line, col) = m.position(at);
        assert_eq!((line, col), (4, 2));
        assert_eq!(m.offset(line, col), at);
        let r = m.view.code.find("go").unwrap();
        let ((l1, c1), (l2, c2)) = m.range(&(r..r + 2));
        assert_eq!((l1, c1, l2, c2), (3, 3, 3, 5));
        let end = m.view.code.len();
        let doc = m.view.range_to_doc(&(end..end));
        assert_eq!(&text[doc.start..doc.start + 3], "]]>");
    }
}
