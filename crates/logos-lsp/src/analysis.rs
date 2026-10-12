//! What the editor features are computed from: a program's sources (the model's code and its
//! `__includes` files), parsed with the primitive table, and an index of where each name is
//! defined. Everything here works on byte offsets in a source's code; the server converts
//! to and from editor positions.

use std::ops::Range;
use std::sync::Arc;

use logos_syntax::{
    Context, FullParse, Language, LocalKind, Owner, Parse, SourceFile, Structure, SyntaxError,
    SyntaxKind, Target, lexer::is_identifier_char, parse, parse_with, segments,
};

/// One source of a program.
#[derive(Debug, Clone)]
pub struct Source {
    /// How the editor names it (its URI), or a path.
    pub name: String,
    pub code: String,
}

/// A place in a program: a source (index into the sources) and a byte range in its code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loc {
    pub source: usize,
    pub range: Range<usize>,
}

/// What a name means, for finding its definition and its uses: two names refer to the same
/// thing exactly when their keys are equal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    Procedure(String),
    Global(String),
    /// An agent variable: `TURTLE`, `PATCH`, `LINK` or a breed's plural name, and the name.
    Variable(String, String),
    /// A local, by the source and offset where it is defined.
    Local(usize, usize),
    /// A breed, by its plural name; its primitives (`create-<breeds>`) refer to it.
    Breed(String),
}

impl Key {
    fn of(target: &Target, source: usize) -> Option<Key> {
        Some(match target {
            Target::Procedure(n) => Key::Procedure(n.clone()),
            Target::Global(n) => Key::Global(n.clone()),
            Target::AgentVariable { owner, name } => {
                let owner = match owner {
                    Owner::Turtle | Owner::TurtleOrLink => "TURTLE".to_owned(),
                    Owner::Patch => "PATCH".to_owned(),
                    Owner::Link => "LINK".to_owned(),
                    Owner::Breed(b) => b.clone(),
                };
                Key::Variable(owner, name.clone())
            }
            Target::Local { definition, .. } => Key::Local(source, definition.start),
            Target::BreedPrimitive { breed, .. } => Key::Breed(breed.clone()),
            Target::Primitive(_) | Target::Constant => return None,
        })
    }
}

/// An outline entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// The whole declaration or procedure.
    pub range: Range<usize>,
    /// The name.
    pub name_range: Range<usize>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Command,
    Reporter,
    Global,
    Breed,
    Variable,
    Extension,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Keyword,
    Command,
    Reporter,
    Variable,
    Constant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: Option<String>,
}

/// A definition site.
#[derive(Debug, Clone)]
struct Def {
    key: Key,
    loc: Loc,
}

enum Parsed {
    Full(FullParse),
    Structure(Parse),
}

impl Parsed {
    fn parse(&self) -> &Parse {
        match self {
            Parsed::Full(f) => &f.parse,
            Parsed::Structure(p) => p,
        }
    }
}

pub struct Analysis {
    pub sources: Vec<Source>,
    pub structure: Structure,
    pub interface_globals: Vec<String>,
    language: Option<Arc<Language>>,
    parsed: Vec<Parsed>,
    defs: Vec<Def>,
}

impl Analysis {
    /// Parses a program. Without a primitive table, only the structure (declarations and
    /// procedures) is parsed, and features that need to know what names in procedure bodies
    /// refer to find nothing.
    pub fn new(
        sources: Vec<Source>,
        interface_globals: Vec<String>,
        language: Option<Arc<Language>>,
    ) -> Analysis {
        let structure = logos_syntax::structure_of(sources.iter().map(|s| s.code.as_str()));
        let parsed: Vec<Parsed> = sources
            .iter()
            .map(|s| match &language {
                Some(lang) => Parsed::Full(parse_with(
                    &s.code,
                    &Context {
                        language: lang,
                        structure: &structure,
                        interface_globals: &interface_globals,
                    },
                )),
                None => Parsed::Structure(parse(&s.code)),
            })
            .collect();
        let mut a = Analysis {
            sources,
            structure,
            interface_globals,
            language,
            parsed,
            defs: Vec::new(),
        };
        a.index_definitions();
        a
    }

    fn file(&self, source: usize) -> SourceFile {
        self.parsed[source].parse().source_file()
    }

    fn index_definitions(&mut self) {
        let mut defs = Vec::new();
        for i in 0..self.sources.len() {
            let file = self.file(i);
            for d in file.declarations() {
                let kw = d.keyword_lower();
                let names = d.names();
                let upper = |t: &logos_syntax::SyntaxToken| t.text().to_ascii_uppercase();
                let range = |t: &logos_syntax::SyntaxToken| -> Range<usize> {
                    t.text_range().start().into()..t.text_range().end().into()
                };
                for (n, t) in names.iter().enumerate() {
                    let key = match kw.as_str() {
                        "globals" => Key::Global(upper(t)),
                        "turtles-own" => Key::Variable("TURTLE".into(), upper(t)),
                        "patches-own" => Key::Variable("PATCH".into(), upper(t)),
                        "links-own" => Key::Variable("LINK".into(), upper(t)),
                        "breed" | "directed-link-breed" | "undirected-link-breed" => {
                            Key::Breed(upper(&names[0]))
                        }
                        "extensions" | "__includes" => continue,
                        k => {
                            Key::Variable(k.trim_end_matches("-own").to_ascii_uppercase(), upper(t))
                        }
                    };
                    let _ = n;
                    defs.push(Def {
                        key,
                        loc: Loc {
                            source: i,
                            range: range(t),
                        },
                    });
                }
            }
            for p in file.procedures() {
                if let (Some(name), Some(range)) = (p.name(), p.name_span()) {
                    defs.push(Def {
                        key: Key::Procedure(name.text().to_ascii_uppercase()),
                        loc: Loc { source: i, range },
                    });
                }
            }
            if let Parsed::Full(f) = &self.parsed[i] {
                for l in &f.locals {
                    defs.push(Def {
                        key: Key::Local(i, l.span.start),
                        loc: Loc {
                            source: i,
                            range: l.span.clone(),
                        },
                    });
                }
            }
        }
        self.defs = defs;
    }

    /// The parser's syntax errors in a source.
    pub fn syntax_errors(&self, source: usize) -> &[SyntaxError] {
        &self.parsed[source].parse().errors
    }

    /// What the name at `offset` means: a use or a definition.
    pub fn key_at(&self, source: usize, offset: usize) -> Option<Key> {
        if let Parsed::Full(f) = &self.parsed[source]
            && let Some(r) = f
                .references
                .iter()
                .find(|r| r.span.start <= offset && offset <= r.span.end)
        {
            return Key::of(&r.target, source);
        }
        self.defs
            .iter()
            .find(|d| {
                d.loc.source == source && d.loc.range.start <= offset && offset <= d.loc.range.end
            })
            .map(|d| d.key.clone())
    }

    pub fn definitions(&self, key: &Key) -> Vec<Loc> {
        self.defs
            .iter()
            .filter(|d| d.key == *key)
            .map(|d| d.loc.clone())
            .collect()
    }

    /// Where the name at `offset` is defined.
    pub fn definition(&self, source: usize, offset: usize) -> Vec<Loc> {
        self.key_at(source, offset)
            .map(|k| self.definitions(&k))
            .unwrap_or_default()
    }

    /// Every use of the name at `offset`, and its definitions if asked.
    pub fn references(&self, source: usize, offset: usize, with_definitions: bool) -> Vec<Loc> {
        let Some(key) = self.key_at(source, offset) else {
            return Vec::new();
        };
        self.uses(&key, with_definitions)
    }

    /// Every use of `key` in the program, and its definitions if asked, in source order.
    pub fn uses(&self, key: &Key, with_definitions: bool) -> Vec<Loc> {
        let mut out = Vec::new();
        for (i, p) in self.parsed.iter().enumerate() {
            if let Parsed::Full(f) = p {
                for r in &f.references {
                    if Key::of(&r.target, i).as_ref() == Some(key) {
                        out.push(Loc {
                            source: i,
                            range: r.span.clone(),
                        });
                    }
                }
            }
        }
        if with_definitions {
            out.extend(self.definitions(key));
        }
        out.sort_by_key(|l| (l.source, l.range.start));
        out.dedup();
        out
    }

    /// Declarations and procedures of a source, in order.
    pub fn outline(&self, source: usize) -> Vec<Symbol> {
        let file = self.file(source);
        let mut out = Vec::new();
        for d in file.declarations() {
            let kw = d.keyword_lower();
            let kind = match kw.as_str() {
                "globals" => SymbolKind::Global,
                "breed" | "directed-link-breed" | "undirected-link-breed" => SymbolKind::Breed,
                "extensions" | "__includes" => SymbolKind::Extension,
                _ => SymbolKind::Variable,
            };
            let names: Vec<String> = d.names().iter().map(|t| t.text().to_owned()).collect();
            let kw_token = d.keyword();
            out.push(Symbol {
                name: kw_token.text().to_owned(),
                kind,
                range: d.span(),
                name_range: kw_token.text_range().start().into()
                    ..kw_token.text_range().end().into(),
                detail: Some(names.join(" ")),
            });
        }
        for p in file.procedures() {
            let (Some(name), Some(name_range)) = (p.name(), p.name_span()) else {
                continue;
            };
            let params: Vec<String> = p.params().iter().map(|t| t.text().to_owned()).collect();
            out.push(Symbol {
                name: name.text().to_owned(),
                kind: if p.is_reporter() {
                    SymbolKind::Reporter
                } else {
                    SymbolKind::Command
                },
                range: p.span(),
                name_range,
                detail: (!params.is_empty()).then(|| format!("[{}]", params.join(" "))),
            });
        }
        out.sort_by_key(|s| s.range.start);
        out
    }

    /// A description of the name at `offset`.
    pub fn hover(&self, source: usize, offset: usize) -> Option<(Range<usize>, String)> {
        let Parsed::Full(f) = &self.parsed[source] else {
            return None;
        };
        let r = f
            .references
            .iter()
            .find(|r| r.span.start <= offset && offset <= r.span.end)?;
        let lang = self.language.as_deref()?;
        let text = match &r.target {
            Target::Primitive(n) => lang.signatures.get(n).cloned().unwrap_or_else(|| n.clone()),
            Target::BreedPrimitive { pattern, breed } => format!(
                "{} (breed {})",
                lang.signatures
                    .get(pattern)
                    .cloned()
                    .unwrap_or_else(|| pattern.clone()),
                breed.to_ascii_lowercase()
            ),
            Target::Procedure(n) => {
                let p = self.structure.procedures.iter().find(|p| p.0 == *n)?;
                let params: Vec<String> = p.2.iter().map(|a| a.to_ascii_lowercase()).collect();
                format!(
                    "{} {}{}",
                    if p.1 { "to-report" } else { "to" },
                    n.to_ascii_lowercase(),
                    if params.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", params.join(" "))
                    }
                )
            }
            Target::Global(n) => {
                if self.interface_globals.contains(n) {
                    format!("global {} (interface)", n.to_ascii_lowercase())
                } else {
                    format!("global {}", n.to_ascii_lowercase())
                }
            }
            Target::AgentVariable { owner, name } => {
                let who = match owner {
                    Owner::Turtle => "turtle".to_owned(),
                    Owner::Patch => "patch".to_owned(),
                    Owner::Link => "link".to_owned(),
                    Owner::TurtleOrLink => "turtle or link".to_owned(),
                    Owner::Breed(b) => b.to_ascii_lowercase(),
                };
                format!("{who} variable {}", name.to_ascii_lowercase())
            }
            Target::Local { name, .. } => {
                let kind = f
                    .locals
                    .iter()
                    .find(|l| l.name == *name)
                    .map(|l| match l.kind {
                        LocalKind::Parameter => "parameter",
                        LocalKind::Let => "local variable",
                        LocalKind::AnonymousArgument => "anonymous procedure argument",
                    })
                    .unwrap_or("local variable");
                format!("{kind} {}", name.to_ascii_lowercase())
            }
            Target::Constant => "constant".to_owned(),
        };
        Some((r.span.clone(), text))
    }

    /// Names that can be written at `offset`, starting with what is already typed there.
    pub fn completions(&self, source: usize, offset: usize) -> Vec<Completion> {
        let code = &self.sources[source].code;
        let offset = offset.min(code.len());
        let mut start = offset;
        while let Some(c) = code[..start].chars().next_back() {
            if !is_identifier_char(c) {
                break;
            }
            start -= c.len_utf8();
        }
        let prefix = code[start..offset].to_ascii_lowercase();
        let mut out: Vec<Completion> = Vec::new();
        let mut add = |label: String, kind: CompletionKind, detail: Option<String>| {
            if label.to_ascii_lowercase().starts_with(&prefix) {
                out.push(Completion {
                    label,
                    kind,
                    detail,
                });
            }
        };
        let lower = |s: &str| s.to_ascii_lowercase();
        if let Some(lang) = self.language.as_deref() {
            for (n, _) in lang.commands.iter().chain(&lang.extension_commands) {
                add(
                    n.clone(),
                    CompletionKind::Command,
                    lang.signatures.get(n).cloned(),
                );
            }
            for (n, _) in lang.reporters.iter().chain(&lang.extension_reporters) {
                add(
                    n.clone(),
                    CompletionKind::Reporter,
                    lang.signatures.get(n).cloned(),
                );
            }
            for n in &lang.constants {
                add(n.clone(), CompletionKind::Constant, None);
            }
            for n in lang
                .turtle_variables
                .iter()
                .chain(&lang.patch_variables)
                .chain(&lang.link_variables)
            {
                add(n.clone(), CompletionKind::Variable, None);
            }
            for b in &self.structure.breeds {
                let plural = lower(&b.name);
                let singular = b.singular.as_deref().map(lower).unwrap_or_default();
                for p in &lang.breed_primitives {
                    let fits = match p.breed_kind {
                        logos_protocol::BreedKind::Turtle => !b.is_link,
                        logos_protocol::BreedKind::DirectedLink => b.is_link && b.directed,
                        logos_protocol::BreedKind::UndirectedLink => b.is_link && !b.directed,
                    };
                    if fits {
                        let name = p
                            .pattern
                            .replace("<breeds>", &plural)
                            .replace("<breed>", &singular);
                        let kind = if p.command {
                            CompletionKind::Command
                        } else {
                            CompletionKind::Reporter
                        };
                        add(name, kind, lang.signatures.get(&p.pattern).cloned());
                    }
                }
            }
            for k in ["to", "to-report", "end"] {
                add(k.to_owned(), CompletionKind::Keyword, None);
            }
        }
        for (name, reporter, params) in &self.structure.procedures {
            let detail = format!(
                "{} {}{}",
                if *reporter { "to-report" } else { "to" },
                lower(name),
                if params.is_empty() {
                    String::new()
                } else {
                    format!(
                        " [{}]",
                        params
                            .iter()
                            .map(|p| lower(p))
                            .collect::<Vec<_>>()
                            .join(" ")
                    )
                }
            );
            let kind = if *reporter {
                CompletionKind::Reporter
            } else {
                CompletionKind::Command
            };
            add(lower(name), kind, Some(detail));
        }
        let s = &self.structure;
        for g in s.globals.iter().chain(&self.interface_globals) {
            add(lower(g), CompletionKind::Variable, Some("global".into()));
        }
        for v in s
            .turtles_own
            .iter()
            .chain(&s.patches_own)
            .chain(&s.links_own)
        {
            add(lower(v), CompletionKind::Variable, None);
        }
        for b in &s.breeds {
            for v in &b.owns {
                add(lower(v), CompletionKind::Variable, Some(lower(&b.name)));
            }
        }
        // Locals defined earlier in the same procedure.
        if let Parsed::Full(f) = &self.parsed[source] {
            let file = self.file(source);
            if let Some(p) = file.procedures().find(|p| {
                let r = p.span();
                r.start <= offset && offset <= r.end
            }) {
                let r = p.span();
                for l in &f.locals {
                    if l.span.start >= r.start && l.span.end <= offset {
                        add(
                            lower(&l.name),
                            CompletionKind::Variable,
                            Some("local".into()),
                        );
                    }
                }
            }
        }
        out.sort_by(|a, b| a.label.cmp(&b.label));
        out.dedup_by(|a, b| a.label == b.label);
        out
    }

    /// The range of the name at `offset` if it can be renamed: a procedure, a global or agent
    /// variable declared in the code, or a local. The error says why not.
    pub fn prepare_rename(&self, source: usize, offset: usize) -> Result<Range<usize>, String> {
        let key = self
            .key_at(source, offset)
            .ok_or("there is no procedure or variable name here")?;
        self.check_renamable(&key)?;
        self.name_range_at(source, offset)
            .ok_or_else(|| "there is no procedure or variable name here".to_owned())
    }

    fn name_range_at(&self, source: usize, offset: usize) -> Option<Range<usize>> {
        if let Parsed::Full(f) = &self.parsed[source]
            && let Some(r) = f
                .references
                .iter()
                .find(|r| r.span.start <= offset && offset <= r.span.end)
        {
            return Some(r.span.clone());
        }
        self.defs
            .iter()
            .find(|d| {
                d.loc.source == source && d.loc.range.start <= offset && offset <= d.loc.range.end
            })
            .map(|d| d.loc.range.clone())
    }

    fn check_renamable(&self, key: &Key) -> Result<(), String> {
        if self.language.is_none() {
            return Err("the NetLogo primitive table isn't loaded yet".into());
        }
        match key {
            Key::Breed(_) => return Err("renaming breeds isn't supported".into()),
            Key::Global(n) if self.interface_globals.contains(n) => {
                return Err(format!(
                    "{} is defined by an interface widget",
                    n.to_ascii_lowercase()
                ));
            }
            Key::Variable(_, n) => {
                let owners: Vec<&Def> = self
                    .defs
                    .iter()
                    .filter(|d| matches!(&d.key, Key::Variable(_, m) if m == n))
                    .collect();
                let distinct: std::collections::HashSet<&Key> =
                    owners.iter().map(|d| &d.key).collect();
                if distinct.len() > 1 {
                    return Err(format!(
                        "{} is declared for more than one kind of agent",
                        n.to_ascii_lowercase()
                    ));
                }
            }
            _ => {}
        }
        if self.definitions(key).is_empty() {
            return Err("this name is built into NetLogo".into());
        }
        Ok(())
    }

    /// The edits that rename what the name at `offset` refers to: every use and definition,
    /// in all sources. Fails if the name can't be renamed or `new_name` isn't a name NetLogo
    /// would accept there. Uses in strings (`run "go"`) are not changed; uses in a model's
    /// widgets and experiments are found with [`Analysis::uses_in_snippet`].
    pub fn rename(&self, source: usize, offset: usize, new_name: &str) -> Result<Vec<Loc>, String> {
        let key = self
            .key_at(source, offset)
            .ok_or("there is no procedure or variable name here")?;
        self.check_renamable(&key)?;
        let (tokens, errors) = logos_syntax::lex(new_name);
        if !errors.is_empty()
            || tokens.len() != 1
            || tokens[0].kind != SyntaxKind::IDENT
            || new_name.is_empty()
        {
            return Err(format!("{new_name:?} isn't a valid NetLogo name"));
        }
        if new_name.contains(['<', '>', '&']) {
            return Err(format!(
                "{new_name} has <, > or &, which Logos can't yet write into a model file"
            ));
        }
        let new_upper = new_name.to_ascii_uppercase();
        let old_upper = match &key {
            Key::Procedure(n) | Key::Global(n) | Key::Variable(_, n) | Key::Breed(n) => n.clone(),
            Key::Local(src, start) => {
                let Parsed::Full(f) = &self.parsed[*src] else {
                    return Err("this name is built into NetLogo".into());
                };
                f.locals
                    .iter()
                    .find(|l| l.span.start == *start)
                    .map(|l| l.name.clone())
                    .unwrap_or_default()
            }
        };
        if new_upper != old_upper && self.name_in_use(&new_upper, &key) {
            return Err(format!(
                "{new_name} is already used in this program or by NetLogo"
            ));
        }
        Ok(self.uses(&key, true))
    }

    /// Where `key` is used in a piece of code kept outside the code section: a button's
    /// commands, a monitor's reporter, a plot's or BehaviorSpace experiment's commands. The
    /// piece is parsed as the body of a command procedure, or as a reported expression when
    /// `reporter` is true. Byte ranges are in `snippet`.
    pub fn uses_in_snippet(&self, snippet: &str, reporter: bool, key: &Key) -> Vec<Range<usize>> {
        let Some(lang) = self.language.as_deref() else {
            return Vec::new();
        };
        let (prefix, suffix) = if reporter {
            ("to-report __logos-snippet report (\n", "\n)\nend\n")
        } else {
            ("to __logos-snippet\n", "\nend\n")
        };
        let code = format!("{prefix}{snippet}{suffix}");
        let f = parse_with(
            &code,
            &Context {
                language: lang,
                structure: &self.structure,
                interface_globals: &self.interface_globals,
            },
        );
        let end = prefix.len() + snippet.len();
        f.references
            .iter()
            .filter(|r| r.span.start >= prefix.len() && r.span.end <= end)
            .filter(|r| Key::of(&r.target, usize::MAX).as_ref() == Some(key))
            .map(|r| r.span.start - prefix.len()..r.span.end - prefix.len())
            .collect()
    }

    /// True if `name` (upper case) means something where `key` is defined: a primitive,
    /// keyword, constant, built-in or declared variable, breed or breed primitive, procedure,
    /// or a local that would clash with it.
    fn name_in_use(&self, name: &str, key: &Key) -> bool {
        let lower = name.to_ascii_lowercase();
        if let Some(lang) = self.language.as_deref() {
            if lang.is_keyword(&lower)
                || lang.constants.contains(&lower)
                || lang.commands.contains_key(&lower)
                || lang.reporters.contains_key(&lower)
                || lang.extension_commands.contains_key(&lower)
                || lang.extension_reporters.contains_key(&lower)
                || lang
                    .turtle_variables
                    .iter()
                    .chain(&lang.patch_variables)
                    .chain(&lang.link_variables)
                    .chain(&lang.observer_variables)
                    .any(|v| v.eq_ignore_ascii_case(name))
            {
                return true;
            }
            for b in &self.structure.breeds {
                let singular = b.singular.as_deref().unwrap_or_default();
                for p in &lang.breed_primitives {
                    let n = p
                        .pattern
                        .replace("<breeds>", &b.name.to_ascii_lowercase())
                        .replace("<breed>", &singular.to_ascii_lowercase());
                    if n == lower {
                        return true;
                    }
                }
            }
        }
        let s = &self.structure;
        let declared = s
            .globals
            .iter()
            .chain(&self.interface_globals)
            .chain(&s.turtles_own)
            .chain(&s.patches_own)
            .chain(&s.links_own)
            .chain(s.breeds.iter().flat_map(|b| b.owns.iter()))
            .chain(s.breeds.iter().map(|b| &b.name))
            .chain(s.breeds.iter().filter_map(|b| b.singular.as_ref()))
            .chain(s.procedures.iter().map(|p| &p.0))
            .any(|n| n == name);
        if declared {
            return true;
        }
        // Locals: for a local, those in the same procedure; for anything else, all of them,
        // since a procedure or global can't share a name with any local.
        let scope = match key {
            Key::Local(src, start) => {
                let file = self.file(*src);
                file.procedures()
                    .map(|p| p.span())
                    .find(|r| r.start <= *start && *start < r.end)
                    .map(|r| (*src, r))
            }
            _ => None,
        };
        self.parsed.iter().enumerate().any(|(i, p)| match p {
            Parsed::Full(f) => f.locals.iter().any(|l| {
                l.name == name
                    && scope.as_ref().is_none_or(|(src, r)| {
                        i == *src && r.start <= l.span.start && l.span.start < r.end
                    })
            }),
            Parsed::Structure(_) => false,
        })
    }

    /// Ranges to grow a selection through, smallest first: the part of a hyphenated or
    /// camel-case name the cursor is in, the whole name, then each enclosing node.
    #[allow(clippy::single_range_in_vec_init)]
    pub fn selection_ranges(&self, source: usize, offset: usize) -> Vec<Range<usize>> {
        let root = self.parsed[source].parse().syntax();
        let code_len = self.sources[source].code.len();
        let offset = offset.min(code_len);
        let mut out: Vec<Range<usize>> = Vec::new();
        let token = root
            .token_at_offset((offset as u32).into())
            .right_biased()
            .filter(|t| !t.kind().is_trivia())
            .or_else(|| root.token_at_offset((offset as u32).into()).left_biased());
        let Some(token) = token else {
            return vec![0..code_len];
        };
        let tstart: usize = token.text_range().start().into();
        let tend: usize = token.text_range().end().into();
        if token.kind() == SyntaxKind::IDENT {
            for seg in segments(token.text()) {
                let r = tstart + seg.start..tstart + seg.end;
                if r.start <= offset && offset <= r.end && r != (tstart..tend) {
                    out.push(r);
                    break;
                }
            }
        }
        out.push(tstart..tend);
        let mut node = token.parent();
        while let Some(n) = node {
            let r: Range<usize> = n.text_range().start().into()..n.text_range().end().into();
            if out.last().is_none_or(|l| *l != r) {
                out.push(r);
            }
            node = n.parent();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODE: &str = "globals [total]\nbreed [frogs frog]\nfrogs-own [age]\n\
to setup\n  create-frogs 3 [ set age 0 ]\n  set total count frogs\nend\n\
to-report older [n]\n  report n + 1\nend\n";

    #[test]
    fn outline_and_structure_without_a_table() {
        let a = Analysis::new(
            vec![Source {
                name: "m".into(),
                code: CODE.into(),
            }],
            vec![],
            None,
        );
        let names: Vec<(String, SymbolKind)> =
            a.outline(0).into_iter().map(|s| (s.name, s.kind)).collect();
        assert_eq!(
            names,
            vec![
                ("globals".into(), SymbolKind::Global),
                ("breed".into(), SymbolKind::Breed),
                ("frogs-own".into(), SymbolKind::Variable),
                ("setup".into(), SymbolKind::Command),
                ("older".into(), SymbolKind::Reporter),
            ]
        );
        // On a definition, the definition is found without a table.
        let at = CODE.find("older").unwrap() + 1;
        let d = a.definition(0, at);
        assert_eq!(&CODE[d[0].range.clone()], "older");
        // Selection grows from a word part to the name to the procedure.
        let at = CODE.find("frogs-own").unwrap() + 1;
        let sel = a.selection_ranges(0, at);
        assert_eq!(&CODE[sel[0].clone()], "frogs");
        assert_eq!(&CODE[sel[1].clone()], "frogs-own");
        let completions: Vec<String> = a
            .completions(0, CODE.find("count").unwrap() + 2)
            .into_iter()
            .map(|c| c.label)
            .collect();
        assert!(completions.is_empty(), "{completions:?}");
    }
}
