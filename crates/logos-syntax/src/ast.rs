//! Typed views of the syntax tree.

use std::ops::Range;

use crate::{SyntaxKind, SyntaxKind::*, SyntaxNode, SyntaxToken};

fn range(node: &SyntaxNode) -> Range<usize> {
    let r = node.text_range();
    usize::from(r.start())..usize::from(r.end())
}

fn token_range(t: &SyntaxToken) -> Range<usize> {
    let r = t.text_range();
    usize::from(r.start())..usize::from(r.end())
}

fn first_token(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == kind)
}

/// The whole parsed source.
#[derive(Debug, Clone)]
pub struct SourceFile(SyntaxNode);

impl SourceFile {
    pub fn cast(node: SyntaxNode) -> Option<SourceFile> {
        (node.kind() == SOURCE_FILE).then_some(SourceFile(node))
    }

    pub fn syntax(&self) -> &SyntaxNode {
        &self.0
    }

    pub fn declarations(&self) -> impl Iterator<Item = Declaration> + '_ {
        self.0
            .children()
            .filter(|n| n.kind() == DECLARATION)
            .map(Declaration)
    }

    pub fn procedures(&self) -> impl Iterator<Item = Procedure> + '_ {
        self.0
            .children()
            .filter(|n| n.kind() == PROCEDURE)
            .map(Procedure)
    }
}

/// A top-level declaration such as `globals [ a b ]`.
#[derive(Debug, Clone)]
pub struct Declaration(SyntaxNode);

impl Declaration {
    pub fn syntax(&self) -> &SyntaxNode {
        &self.0
    }

    /// The keyword as written, for example `globals` or `wolves-own`.
    pub fn keyword(&self) -> SyntaxToken {
        first_token(&self.0, IDENT).expect("a declaration starts with its keyword")
    }

    /// The keyword in lower case.
    pub fn keyword_lower(&self) -> String {
        self.keyword().text().to_ascii_lowercase()
    }

    /// The names (identifiers, or strings for `__includes`) in the list, as written.
    pub fn names(&self) -> Vec<SyntaxToken> {
        self.0
            .children()
            .find(|n| n.kind() == NAME_LIST)
            .map(|l| {
                l.children_with_tokens()
                    .filter_map(|e| e.into_token())
                    .filter(|t| t.kind() == IDENT || t.kind() == STRING)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn span(&self) -> Range<usize> {
        range(&self.0)
    }
}

/// `to` / `to-report` ... `end`.
#[derive(Debug, Clone)]
pub struct Procedure(SyntaxNode);

impl Procedure {
    pub fn syntax(&self) -> &SyntaxNode {
        &self.0
    }

    pub fn is_reporter(&self) -> bool {
        first_token(&self.0, IDENT).is_some_and(|t| t.text().eq_ignore_ascii_case("to-report"))
    }

    pub fn name(&self) -> Option<SyntaxToken> {
        self.0
            .children()
            .find(|n| n.kind() == PROCEDURE_NAME)
            .and_then(|n| first_token(&n, IDENT))
    }

    pub fn params(&self) -> Vec<SyntaxToken> {
        self.0
            .children()
            .find(|n| n.kind() == PARAM_LIST)
            .map(|l| {
                l.children_with_tokens()
                    .filter_map(|e| e.into_token())
                    .filter(|t| t.kind() == IDENT)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn body(&self) -> Option<SyntaxNode> {
        self.0.children().find(|n| n.kind() == BODY)
    }

    /// Whether the procedure ends with `end`.
    pub fn has_end(&self) -> bool {
        self.0
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .any(|t| t.kind() == IDENT && t.text().eq_ignore_ascii_case("end"))
    }

    pub fn span(&self) -> Range<usize> {
        range(&self.0)
    }

    pub fn name_span(&self) -> Option<Range<usize>> {
        self.name().map(|t| token_range(&t))
    }
}

/// A breed or link breed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breed {
    pub name: String,
    pub singular: Option<String>,
    pub is_link: bool,
    pub directed: bool,
    pub owns: Vec<String>,
}

/// A model's declarations and procedures, with names in upper case as NetLogo's `Program`
/// stores them. Built from one or more sources (the code section and its `__includes` files).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Structure {
    pub globals: Vec<String>,
    pub turtles_own: Vec<String>,
    pub patches_own: Vec<String>,
    pub links_own: Vec<String>,
    /// Turtle breeds, then link breeds, each in declaration order.
    pub breeds: Vec<Breed>,
    pub extensions: Vec<String>,
    /// File names as written, without quotes.
    pub includes: Vec<String>,
    /// (name, is reporter, parameters).
    pub procedures: Vec<(String, bool, Vec<String>)>,
}

fn upper(s: &str) -> String {
    s.to_ascii_uppercase()
}

impl Structure {
    /// Adds the declarations and procedures of one source.
    pub fn add(&mut self, file: &SourceFile) {
        // `-own` declarations can come before or after their breed, so collect them first.
        let mut owns: Vec<(String, Vec<String>)> = Vec::new();
        for d in file.declarations() {
            let names: Vec<String> = d.names().iter().map(|t| upper(t.text())).collect();
            match d.keyword_lower().as_str() {
                "globals" => self.globals.extend(names),
                "turtles-own" => self.turtles_own.extend(names),
                "patches-own" => self.patches_own.extend(names),
                "links-own" => self.links_own.extend(names),
                "extensions" => self.extensions.extend(names),
                "__includes" => self.includes.extend(
                    d.names()
                        .iter()
                        .map(|t| t.text().trim_matches('"').to_owned()),
                ),
                k @ ("breed" | "directed-link-breed" | "undirected-link-breed") => {
                    let mut it = names.into_iter();
                    if let Some(name) = it.next() {
                        self.breeds.push(Breed {
                            name,
                            singular: it.next(),
                            is_link: k != "breed",
                            directed: k == "directed-link-breed",
                            owns: Vec::new(),
                        });
                    }
                }
                k => {
                    let breed = k.trim_end_matches("-own");
                    owns.push((upper(breed), names));
                }
            }
        }
        for (breed, names) in owns {
            if let Some(b) = self.breeds.iter_mut().find(|b| b.name == breed) {
                b.owns.extend(names);
            }
        }
        // Turtle breeds before link breeds, as NetLogo's Program keeps them in two lists.
        self.breeds.sort_by_key(|b| b.is_link);
        for p in file.procedures() {
            if let Some(name) = p.name() {
                self.procedures.push((
                    upper(name.text()),
                    p.is_reporter(),
                    p.params().iter().map(|t| upper(t.text())).collect(),
                ));
            }
        }
    }
}
