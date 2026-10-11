//! The structure parser. It builds a rowan green tree from the token list and records errors;
//! every token goes into the tree exactly once.

use std::ops::Range;

use rowan::{GreenNode, GreenNodeBuilder};

use crate::lexer::Token;
use crate::{SyntaxError, SyntaxKind, SyntaxKind::*};

pub(crate) fn parse(source: &str, tokens: &[Token]) -> (GreenNode, Vec<SyntaxError>) {
    let mut p = Parser {
        source,
        tokens,
        pos: 0,
        builder: GreenNodeBuilder::new(),
        errors: Vec::new(),
    };
    p.source_file();
    (p.builder.finish(), p.errors)
}

struct Parser<'a> {
    source: &'a str,
    tokens: &'a [Token],
    pos: usize,
    builder: GreenNodeBuilder<'static>,
    errors: Vec<SyntaxError>,
}

/// Keywords that start a declaration at the top level.
fn is_declaration_keyword(word: &str) -> bool {
    matches!(
        word,
        "globals"
            | "extensions"
            | "__includes"
            | "breed"
            | "directed-link-breed"
            | "undirected-link-breed"
    ) || (word.ends_with("-own") && word.len() > "-own".len())
}

impl Parser<'_> {
    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn kind(&self) -> Option<SyntaxKind> {
        self.tokens.get(self.pos).map(|t| t.kind)
    }

    fn span(&self) -> Range<usize> {
        self.tokens[self.pos].span.clone()
    }

    /// The current token's text in lower case, if it is an identifier.
    fn word(&self) -> Option<String> {
        let t = self.tokens.get(self.pos)?;
        (t.kind == IDENT).then(|| self.source[t.span.clone()].to_ascii_lowercase())
    }

    /// The kind of the next non-trivia token after the current one.
    fn peek_kind(&self) -> Option<SyntaxKind> {
        self.tokens[self.pos + 1..]
            .iter()
            .map(|t| t.kind)
            .find(|k| !k.is_trivia())
    }

    fn bump(&mut self) {
        let t = &self.tokens[self.pos];
        self.builder
            .token(t.kind.into(), &self.source[t.span.clone()]);
        self.pos += 1;
    }

    fn eat_trivia(&mut self) {
        while self.kind().is_some_and(SyntaxKind::is_trivia) {
            self.bump();
        }
    }

    fn error(&mut self, span: Range<usize>, message: impl Into<String>) {
        self.errors.push(SyntaxError {
            span,
            message: message.into(),
        });
    }

    fn at_procedure_start(&self) -> bool {
        matches!(self.word().as_deref(), Some("to" | "to-report"))
    }

    /// A declaration keyword followed by `[`. `breed` alone is also a turtle variable, so
    /// inside procedure bodies only the bracketed form ends a procedure that lacks `end`.
    fn at_declaration_start(&self) -> bool {
        self.word().is_some_and(|w| is_declaration_keyword(&w))
            && self.peek_kind() == Some(L_BRACKET)
    }

    fn source_file(&mut self) {
        self.builder.start_node(SOURCE_FILE.into());
        loop {
            self.eat_trivia();
            if self.at_end() {
                break;
            }
            if self.at_procedure_start() {
                self.procedure();
            } else if self.word().is_some_and(|w| is_declaration_keyword(&w)) {
                self.declaration();
            } else {
                self.top_level_error();
            }
        }
        self.builder.finish_node();
    }

    /// Code outside any declaration or procedure, up to the next declaration or `to`.
    fn top_level_error(&mut self) {
        self.builder.start_node(ERROR.into());
        let start = self.span().start;
        let mut end = start;
        while !self.at_end() {
            if self.at_procedure_start() || self.word().is_some_and(|w| is_declaration_keyword(&w))
            {
                break;
            }
            if !self.kind().unwrap().is_trivia() {
                end = self.span().end;
            }
            self.bump();
        }
        self.builder.finish_node();
        self.error(start..end, "expected a declaration or TO / TO-REPORT");
    }

    fn declaration(&mut self) {
        self.builder.start_node(DECLARATION.into());
        let keyword = self.span();
        let word = self.word().unwrap();
        self.bump();
        self.eat_trivia();
        if self.kind() == Some(L_BRACKET) {
            self.name_list(NAME_LIST, word == "__includes");
        } else {
            self.error(keyword, format!("expected [ after {}", word.to_uppercase()));
        }
        self.builder.finish_node();
    }

    /// `[ name ... ]` for a declaration (`strings` for `__includes`) or a parameter list.
    fn name_list(&mut self, kind: SyntaxKind, strings: bool) {
        self.builder.start_node(kind.into());
        let open = self.span();
        self.bump();
        loop {
            self.eat_trivia();
            if self.at_end() || self.at_procedure_start() || self.at_declaration_start() {
                self.error(open, "missing ]");
                break;
            }
            match self.kind().unwrap() {
                R_BRACKET => {
                    self.bump();
                    break;
                }
                IDENT if !strings => self.bump(),
                STRING if strings => self.bump(),
                _ => {
                    let span = self.span();
                    self.builder.start_node(ERROR.into());
                    self.bump();
                    self.builder.finish_node();
                    self.error(
                        span,
                        if strings {
                            "expected a file name"
                        } else {
                            "expected a name"
                        },
                    );
                }
            }
        }
        self.builder.finish_node();
    }

    fn procedure(&mut self) {
        self.builder.start_node(PROCEDURE.into());
        let header_start = self.span().start;
        let mut header_end = self.span().end;
        self.bump();
        self.eat_trivia();
        let named = self.kind() == Some(IDENT)
            && !self.at_procedure_start()
            && self.word().as_deref() != Some("end");
        if named {
            header_end = self.span().end;
            self.builder.start_node(PROCEDURE_NAME.into());
            self.bump();
            self.builder.finish_node();
        } else {
            self.error(header_start..header_end, "expected a procedure name");
        }
        // Trivia after the header stays in the procedure; the body starts after it.
        self.eat_trivia();
        if self.kind() == Some(L_BRACKET) && named {
            self.name_list(PARAM_LIST, false);
        }
        self.body(header_start..header_end);
        self.builder.finish_node();
    }

    fn body(&mut self, header: Range<usize>) {
        self.builder.start_node(BODY.into());
        // Open groups: kind of group, span of its opening token.
        let mut open: Vec<(SyntaxKind, Range<usize>)> = Vec::new();
        loop {
            if self.at_end() {
                self.close_groups(&mut open, 0);
                self.builder.finish_node();
                self.error(header, "missing END");
                return;
            }
            let ends_here = self.word().as_deref() == Some("end");
            let next_starts =
                self.at_procedure_start() || (open.is_empty() && self.at_declaration_start());
            if ends_here || next_starts {
                self.close_groups(&mut open, 0);
                self.builder.finish_node();
                if ends_here {
                    self.bump();
                } else {
                    self.error(header, "missing END");
                }
                return;
            }
            match self.kind().unwrap() {
                L_BRACKET | L_PAREN => {
                    let k = self.kind().unwrap();
                    open.push((k, self.span()));
                    self.builder.start_node(
                        if k == L_BRACKET {
                            BRACKET_GROUP
                        } else {
                            PAREN_GROUP
                        }
                        .into(),
                    );
                    self.bump();
                }
                R_BRACKET | R_PAREN => {
                    let opener = if self.kind() == Some(R_BRACKET) {
                        L_BRACKET
                    } else {
                        L_PAREN
                    };
                    match open.iter().rposition(|(k, _)| *k == opener) {
                        Some(i) => {
                            self.close_groups(&mut open, i + 1);
                            self.bump();
                            self.builder.finish_node();
                            open.pop();
                        }
                        None => {
                            let span = self.span();
                            let text = self.source[span.clone()].to_owned();
                            self.builder.start_node(ERROR.into());
                            self.bump();
                            self.builder.finish_node();
                            self.error(span, format!("{text} without a matching opening bracket"));
                        }
                    }
                }
                _ => self.bump(),
            }
        }
    }

    /// Closes open groups down to `keep` entries, reporting each as unclosed.
    fn close_groups(&mut self, open: &mut Vec<(SyntaxKind, Range<usize>)>, keep: usize) {
        while open.len() > keep {
            let (k, span) = open.pop().unwrap();
            self.builder.finish_node();
            self.error(
                span,
                if k == L_BRACKET {
                    "missing ]"
                } else {
                    "missing )"
                },
            );
        }
    }
}
