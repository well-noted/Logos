//! The expression parser for procedure bodies (Phase 1, Part S).
//!
//! It follows the behavior of NetLogo 7.0.4's compiler front end, as described in the NetLogo
//! documentation and observed against the engine: every name is classified (keyword,
//! constant, primitive, breed primitive, variable, procedure argument, procedure, or unknown),
//! commands take exactly their default number of arguments unless the whole statement is in
//! parentheses, each argument is an expression parsed at the precedence of the command or
//! reporter it belongs to, infix reporters bind by precedence, and a bracketed block is only
//! interpreted once the type expected in its position is known: a command block, a reporter
//! block, an anonymous procedure, a literal list, or a code block. A primitive's name alone
//! where an anonymous procedure is expected is the concise form (`foreach xs print`).
//!
//! Parsing goes in two steps. The first builds nodes over token indices and records
//! references and local definitions; blocks wait as token ranges until their type is known.
//! The second writes the nodes into the rowan tree, with every token (comments and whitespace
//! included) in source order, so the tree stays lossless.

use std::collections::HashMap;
use std::ops::Range;

use rowan::GreenNodeBuilder;

use crate::ast::Structure;
use crate::language::{Language, Masks, Syntax};
use crate::lexer::Token;
use crate::{SyntaxError, SyntaxKind, SyntaxKind::*};
use logos_protocol::BreedKind;

/// What a name in the code refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A NetLogo primitive (lower case), including extension primitives such as `nw:save`.
    Primitive(String),
    /// A primitive derived from a breed: the pattern (`create-<breeds>`) and the breed's plural
    /// name in upper case.
    BreedPrimitive { pattern: String, breed: String },
    /// A procedure, by its name in upper case.
    Procedure(String),
    /// A global variable, from `globals` or an interface widget.
    Global(String),
    /// An agent variable: whose it is and its name in upper case.
    AgentVariable { owner: Owner, name: String },
    /// A procedure argument, `let` variable or anonymous procedure argument, with the span of
    /// the name where it is defined.
    Local {
        name: String,
        definition: Range<usize>,
    },
    /// `true`, `false`, `nobody`, `pi`, `e` or a color name.
    Constant,
}

/// Which agents own an agent variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    Turtle,
    Patch,
    Link,
    /// Declared in both `turtles-own` and `links-own`.
    TurtleOrLink,
    /// A turtle breed's or link breed's own variable; the breed's plural name in upper case.
    Breed(String),
}

/// A name used in the code and what it refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub span: Range<usize>,
    pub target: Target,
}

/// Where a local name is defined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalKind {
    Parameter,
    Let,
    AnonymousArgument,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDefinition {
    /// Upper case.
    pub name: String,
    pub span: Range<usize>,
    pub kind: LocalKind,
}

/// The program around the code being parsed: its declarations and procedures (from all of its
/// sources) and the globals its interface widgets define.
pub struct Context<'a> {
    pub language: &'a Language,
    pub structure: &'a Structure,
    /// Upper case.
    pub interface_globals: &'a [String],
}

/// How a name is classified before parsing, in NetLogo's order.
#[derive(Debug, Clone)]
enum Class {
    Keyword,
    Literal,
    Command(Syntax, Target),
    Reporter(Syntax, Target),
    /// Not known to the program; may be a local variable.
    Unknown,
}

/// The program's names, indexed once per context.
pub(crate) struct Names<'a> {
    lang: &'a Language,
    breed_prims: HashMap<String, (bool, Syntax, Target)>,
    variables: HashMap<String, Target>,
    procedures: HashMap<String, (bool, usize)>,
}

impl<'a> Names<'a> {
    pub(crate) fn new(ctx: &Context<'a>) -> Names<'a> {
        let lang = ctx.language;
        let s = ctx.structure;
        let mut breed_prims = HashMap::new();
        for b in &s.breeds {
            let kind = match (b.is_link, b.directed) {
                (false, _) => BreedKind::Turtle,
                (true, true) => BreedKind::DirectedLink,
                (true, false) => BreedKind::UndirectedLink,
            };
            let plural = b.name.to_ascii_lowercase();
            let singular = b.singular.as_deref().unwrap_or("").to_ascii_lowercase();
            for p in lang
                .breed_primitives
                .iter()
                .filter(|p| p.breed_kind == kind)
            {
                if p.pattern.contains("<breed>") && singular.is_empty() {
                    continue;
                }
                let name = p
                    .pattern
                    .replace("<breeds>", &plural)
                    .replace("<breed>", &singular);
                breed_prims.entry(name).or_insert_with(|| {
                    (
                        p.command,
                        p.syntax.clone(),
                        Target::BreedPrimitive {
                            pattern: p.pattern.clone(),
                            breed: b.name.clone(),
                        },
                    )
                });
            }
        }
        // Agent variables, in the order NetLogo looks them up: breed variables, link breed
        // variables, variables both turtles and links own, then turtle, patch, link and
        // observer variables.
        let mut variables: HashMap<String, Target> = HashMap::new();
        let mut add = |name: &str, t: Target| {
            variables.entry(name.to_ascii_lowercase()).or_insert(t);
        };
        for b in s.breeds.iter().filter(|b| !b.is_link) {
            for v in &b.owns {
                add(v, var(Owner::Breed(b.name.clone()), v));
            }
        }
        for b in s.breeds.iter().filter(|b| b.is_link) {
            for v in &b.owns {
                add(v, var(Owner::Breed(b.name.clone()), v));
            }
        }
        for v in &s.turtles_own {
            if s.links_own.contains(v) {
                add(v, var(Owner::TurtleOrLink, v));
            }
        }
        for v in lang.turtle_variables.iter().chain(&s.turtles_own) {
            add(v, var(Owner::Turtle, v));
        }
        for v in lang.patch_variables.iter().chain(&s.patches_own) {
            add(v, var(Owner::Patch, v));
        }
        for v in lang.link_variables.iter().chain(&s.links_own) {
            add(v, var(Owner::Link, v));
        }
        for v in lang
            .observer_variables
            .iter()
            .chain(&s.globals)
            .chain(ctx.interface_globals)
        {
            add(v, Target::Global(v.to_ascii_uppercase()));
        }
        let procedures = s
            .procedures
            .iter()
            .map(|(name, reporter, params)| (name.to_ascii_lowercase(), (*reporter, params.len())))
            .collect();
        Names {
            lang,
            breed_prims,
            variables,
            procedures,
        }
    }

    /// Classifies a name, with the current procedure's parameters.
    fn classify(&self, lower: &str, params: &[(String, Range<usize>)]) -> Class {
        let lang = self.lang;
        let m = &lang.masks;
        if lang.is_keyword(lower) {
            return Class::Keyword;
        }
        if lang.constants.contains(lower) {
            return Class::Literal;
        }
        if let Some(s) = lang.commands.get(lower) {
            return Class::Command(s.clone(), Target::Primitive(lower.to_owned()));
        }
        if let Some(s) = lang.reporters.get(lower) {
            return Class::Reporter(s.clone(), Target::Primitive(lower.to_owned()));
        }
        if let Some((command, s, t)) = self.breed_prims.get(lower) {
            return if *command {
                Class::Command(s.clone(), t.clone())
            } else {
                Class::Reporter(s.clone(), t.clone())
            };
        }
        if let Some(t) = self.variables.get(lower) {
            return Class::Reporter(Syntax::procedure(0, m, false), t.clone());
        }
        if let Some(s) = lang.extension_commands.get(lower) {
            return Class::Command(s.clone(), Target::Primitive(lower.to_owned()));
        }
        if let Some(s) = lang.extension_reporters.get(lower) {
            return Class::Reporter(s.clone(), Target::Primitive(lower.to_owned()));
        }
        let upper = lower.to_ascii_uppercase();
        if let Some((_, span)) = params.iter().find(|(n, _)| *n == upper) {
            return Class::Reporter(
                Syntax::procedure(0, m, false),
                Target::Local {
                    name: upper,
                    definition: span.clone(),
                },
            );
        }
        if let Some((reporter, n)) = self.procedures.get(lower) {
            return if *reporter {
                Class::Reporter(Syntax::procedure(*n, m, false), Target::Procedure(upper))
            } else {
                Class::Command(Syntax::procedure(*n, m, true), Target::Procedure(upper))
            };
        }
        Class::Unknown
    }
}

fn var(owner: Owner, name: &str) -> Target {
    Target::AgentVariable {
        owner,
        name: name.to_ascii_uppercase(),
    }
}

// ---------------------------------------------------------------- intermediate nodes

#[derive(Debug, Clone, Copy)]
enum Part {
    /// A token, by index into the token list.
    Tok(usize),
    Node(usize),
}

#[derive(Debug)]
struct Node {
    kind: SyntaxKind,
    parts: Vec<Part>,
}

/// Local names in scope: upper-case name and definition span.
type Scope = Vec<(String, Range<usize>)>;

/// An expression, or a bracketed block waiting for its expected type.
enum Expr {
    Node(usize),
    Delayed(Delayed),
}

struct Delayed {
    /// Positions (in `sig`) of `[` and of its matching `]` (or the limit, if unclosed).
    open: usize,
    close: usize,
    closed: bool,
    scope: Scope,
}

/// A parse failure that the caller interprets, as NetLogo does.
#[derive(Debug, Clone, Copy)]
enum Fail {
    /// A token that can't start an expression here.
    Unexpected(usize),
    /// An infix reporter with nothing on its left.
    MissingPrefix(usize),
    /// Already reported; recover.
    Reported,
}

const MIN_PRECEDENCE: i32 = -1;

struct BodyParser<'a, 'n> {
    source: &'a str,
    tokens: &'a [Token],
    /// Indices of the body's non-trivia tokens.
    sig: Vec<usize>,
    pos: usize,
    /// Parsing stops here (the closing bracket of the block being parsed, or the body's end).
    limit: usize,
    names: &'a Names<'n>,
    params: &'a [(String, Range<usize>)],
    masks: Masks,
    nodes: Vec<Node>,
    errors: Vec<SyntaxError>,
    references: Vec<Reference>,
    locals: Vec<LocalDefinition>,
    /// Classification of each token in `sig`, computed on first use.
    classes: HashMap<usize, Class>,
    /// Local names in scope here (`let` variables and anonymous procedure arguments).
    scope: Scope,
}

/// The result of parsing one procedure body.
pub(crate) struct BodyResult {
    pub errors: Vec<SyntaxError>,
    pub references: Vec<Reference>,
    pub locals: Vec<LocalDefinition>,
}

/// Parses the tokens `tokens[range]` as a procedure body and writes them into `builder`.
pub(crate) fn parse_body(
    source: &str,
    tokens: &[Token],
    range: Range<usize>,
    names: &Names,
    params: &[(String, Range<usize>)],
    builder: &mut GreenNodeBuilder<'static>,
) -> BodyResult {
    let sig: Vec<usize> = range
        .clone()
        .filter(|&i| !tokens[i].kind.is_trivia())
        .collect();
    let mut p = BodyParser {
        source,
        tokens,
        limit: sig.len(),
        sig,
        pos: 0,
        names,
        params,
        masks: names.lang.masks,
        nodes: Vec::new(),
        errors: Vec::new(),
        references: Vec::new(),
        locals: Vec::new(),
        classes: HashMap::new(),
        scope: Vec::new(),
    };
    let top = p.statements();
    // Write the tree: the statements in order, with every token of the range.
    let mut cursor = range.start;
    for n in top {
        p.emit_node(n, &mut cursor, builder);
    }
    p.emit_until(range.end, &mut cursor, builder);
    BodyResult {
        errors: p.errors,
        references: p.references,
        locals: p.locals,
    }
}

impl BodyParser<'_, '_> {
    // ------------------------------------------------------------ tokens

    fn at_limit(&self) -> bool {
        self.pos >= self.limit
    }

    fn tok(&self, pos: usize) -> &Token {
        &self.tokens[self.sig[pos]]
    }

    fn kind_at(&self, pos: usize) -> Option<SyntaxKind> {
        (pos < self.limit).then(|| self.tok(pos).kind)
    }

    fn kind(&self) -> Option<SyntaxKind> {
        self.kind_at(self.pos)
    }

    fn text(&self, pos: usize) -> &str {
        &self.source[self.tok(pos).span.clone()]
    }

    fn span(&self, pos: usize) -> Range<usize> {
        self.tok(pos).span.clone()
    }

    fn class(&mut self, pos: usize) -> Class {
        if let Some(c) = self.classes.get(&pos) {
            return c.clone();
        }
        let c = match self.tok(pos).kind {
            NUMBER | STRING | EXTENSION_LITERAL => Class::Literal,
            IDENT => {
                let lower = self.text(pos).to_ascii_lowercase();
                self.names.classify(&lower, self.params)
            }
            _ => Class::Unknown,
        };
        self.classes.insert(pos, c.clone());
        c
    }

    fn is_command(&mut self, pos: usize) -> bool {
        pos < self.limit
            && self.tok(pos).kind == IDENT
            && matches!(self.class(pos), Class::Command(..))
    }

    fn node(&mut self, kind: SyntaxKind, parts: Vec<Part>) -> usize {
        self.nodes.push(Node { kind, parts });
        self.nodes.len() - 1
    }

    fn error(&mut self, span: Range<usize>, message: impl Into<String>) {
        self.errors.push(SyntaxError {
            span,
            message: message.into(),
        });
    }

    fn reference(&mut self, pos: usize, target: Target) {
        let span = self.span(pos);
        self.references.push(Reference { span, target });
    }

    fn tok_part(&self, pos: usize) -> Part {
        Part::Tok(self.sig[pos])
    }

    /// Wraps the tokens from `from` up to the current position (and at least one token) in an
    /// error node.
    fn error_node(&mut self, from: usize) -> usize {
        let end = self.pos.max(from + 1).min(self.limit.max(from + 1));
        let parts = (from..end.min(self.sig.len()))
            .map(|i| self.tok_part(i))
            .collect();
        self.pos = end;
        self.node(ERROR, parts)
    }

    // ------------------------------------------------------------ statements

    /// Statements up to the limit. Each statement's `let` adds to the scope of those after it,
    /// up to the end of the list.
    fn statements(&mut self) -> Vec<usize> {
        let saved = self.scope.clone();
        let mut out = Vec::new();
        while !self.at_limit() {
            let start = self.pos;
            match self.statement(false) {
                Ok(n) => out.push(n),
                Err(f) => {
                    self.report(f, start, "Expected command.");
                    // Skip to the next command, so one error doesn't hide the rest.
                    self.pos = self.pos.max(start + 1);
                    while !self.at_limit()
                        && !self.is_command(self.pos)
                        && self.kind() != Some(L_PAREN)
                    {
                        if self.kind() == Some(L_BRACKET) {
                            self.pos = self.matching_close(self.pos).0 + 1;
                        } else {
                            self.pos += 1;
                        }
                    }
                    self.pos = self.pos.min(self.limit);
                    let n = self.error_node(start);
                    out.push(n);
                }
            }
        }
        self.scope = saved;
        out
    }

    /// Records the error for a failure, with NetLogo's wording where the failure says nothing
    /// more specific.
    fn report(&mut self, f: Fail, start: usize, expected: &str) {
        match f {
            Fail::Reported => {}
            Fail::MissingPrefix(p) => {
                let span = self.span(p);
                self.error(span, "Missing input on the left.");
            }
            Fail::Unexpected(p) => {
                let p = p
                    .min(self.limit.saturating_sub(1))
                    .max(start.min(self.sig.len() - 1));
                let span = self.span(p);
                let kind = self.tok(p).kind;
                let message = match kind {
                    R_PAREN => "Unexpected closing parenthesis.".to_owned(),
                    R_BRACKET => "Unexpected closing bracket.".to_owned(),
                    IDENT if matches!(self.class(p), Class::Unknown) => format!(
                        "Nothing named {} has been defined.",
                        self.text(p).to_ascii_uppercase()
                    ),
                    _ => expected.to_owned(),
                };
                self.error(span, message);
            }
        }
    }

    fn statement(&mut self, variadic: bool) -> Result<usize, Fail> {
        if self.at_limit() {
            return Err(Fail::Unexpected(self.pos));
        }
        if self.kind() == Some(L_PAREN) {
            let open = self.pos;
            self.pos += 1;
            let inner = self.statement(true)?;
            return self.close_paren(open, inner);
        }
        let head = self.pos;
        let Class::Command(syntax, target) = self.class(head) else {
            return Err(Fail::Unexpected(head));
        };
        self.reference(head, target.clone());
        self.pos += 1;
        let mut parts = vec![self.tok_part(head)];
        let is_prim = |n: &str| target == Target::Primitive(n.to_owned());
        if is_prim("let") {
            // The name (or a bracketed list of names) comes into scope before the value.
            let names = self.let_names()?;
            parts.push(Part::Node(names));
            let value = self.argument(&syntax, self.masks.wildcard)?;
            parts.push(Part::Node(self.resolve(value, self.masks.wildcard)));
        } else if is_prim("set") && self.kind() == Some(L_BRACKET) {
            let targets = self.set_names()?;
            parts.push(Part::Node(targets));
            let value = self.argument(&syntax, self.masks.wildcard)?;
            parts.push(Part::Node(self.resolve(value, self.masks.wildcard)));
        } else {
            let args = self.arguments(&syntax, variadic)?;
            parts.extend(args);
        }
        Ok(self.node(STATEMENT, parts))
    }

    fn close_paren(&mut self, open: usize, inner: usize) -> Result<usize, Fail> {
        if self.kind() == Some(R_PAREN) {
            let parts = vec![
                self.tok_part(open),
                Part::Node(inner),
                self.tok_part(self.pos),
            ];
            self.pos += 1;
            Ok(self.node(PARENS, parts))
        } else {
            let (span, message) = if self.at_limit() {
                (
                    self.span(open),
                    "No closing parenthesis for this open parenthesis.",
                )
            } else {
                (self.span(self.pos), "Expected a closing parenthesis here.")
            };
            self.error(span, message);
            let parts = vec![self.tok_part(open), Part::Node(inner)];
            Ok(self.node(PARENS, parts))
        }
    }

    /// The name after `let`, or a bracketed (possibly nested) list of names.
    fn let_names(&mut self) -> Result<usize, Fail> {
        match self.kind() {
            Some(L_BRACKET) => {
                let open = self.pos;
                self.pos += 1;
                let mut parts = vec![self.tok_part(open)];
                loop {
                    match self.kind() {
                        None => {
                            let span = self.span(open);
                            self.error(span, "No closing bracket for this open bracket.");
                            break;
                        }
                        Some(R_BRACKET) => {
                            parts.push(self.tok_part(self.pos));
                            self.pos += 1;
                            break;
                        }
                        Some(L_BRACKET) => {
                            let inner = self.let_names()?;
                            parts.push(Part::Node(inner));
                        }
                        Some(IDENT) => {
                            let n = self.define(LocalKind::Let);
                            parts.push(Part::Node(n));
                        }
                        Some(_) => return Err(Fail::Unexpected(self.pos)),
                    }
                }
                Ok(self.node(NAMES, parts))
            }
            // The name after `let` is a new name whatever else it could mean. Later uses are
            // classified as usual, so a name like `holes-here` still means the breed
            // primitive when used.
            Some(IDENT) => Ok(self.define(LocalKind::Let)),
            _ => {
                let span = self.span(self.pos.min(self.limit.max(1) - 1));
                self.error(span, "Expected variable name here");
                Err(Fail::Reported)
            }
        }
    }

    /// A name being defined at the current token.
    fn define(&mut self, kind: LocalKind) -> usize {
        let pos = self.pos;
        let name = self.text(pos).to_ascii_uppercase();
        let span = self.span(pos);
        self.scope.retain(|(n, _)| *n != name);
        self.scope.push((name.clone(), span.clone()));
        self.locals.push(LocalDefinition { name, span, kind });
        self.pos += 1;
        let part = self.tok_part(pos);
        self.node(NAME, vec![part])
    }

    /// `set [a [b c]] ...`: the variables to set, as names.
    fn set_names(&mut self) -> Result<usize, Fail> {
        let open = self.pos;
        self.pos += 1;
        let mut parts = vec![self.tok_part(open)];
        loop {
            match self.kind() {
                None => {
                    let span = self.span(open);
                    self.error(span, "No closing bracket for this open bracket.");
                    break;
                }
                Some(R_BRACKET) => {
                    parts.push(self.tok_part(self.pos));
                    self.pos += 1;
                    break;
                }
                Some(L_BRACKET) => {
                    let inner = self.set_names()?;
                    parts.push(Part::Node(inner));
                }
                Some(_) => {
                    let pos = self.pos;
                    if let Class::Reporter(_, t) = self.class(pos) {
                        self.reference(pos, t);
                    }
                    self.pos += 1;
                    let part = self.tok_part(pos);
                    let n = self.node(REPORTER_APP, vec![part]);
                    parts.push(Part::Node(n));
                }
            }
        }
        Ok(self.node(NAMES, parts))
    }

    // ------------------------------------------------------------ arguments

    /// The arguments of a command or reporter: in parentheses (`variadic`), any number up to
    /// the closing parenthesis; otherwise exactly the default number, plus an optional
    /// command block if one follows.
    fn arguments(&mut self, syntax: &Syntax, variadic: bool) -> Result<Vec<Part>, Fail> {
        let m = self.masks;
        let mut out = Vec::new();
        if variadic && syntax.is_variadic(&m) {
            let mut goal = 0usize;
            loop {
                match self.kind() {
                    None | Some(R_PAREN) => break,
                    _ => {}
                }
                if let Some(IDENT) = self.kind()
                    && let Class::Reporter(s, _) = self.class(self.pos)
                    && s.is_infix()
                    && syntax.right.get(goal).copied() != Some(m.reporter)
                {
                    // An infix reporter ends the argument list; the parentheses then hold an
                    // expression, which only works with the default number of arguments.
                    if out.len() != syntax.total_default(&m) - usize::from(syntax.is_infix()) {
                        let span = self.span(self.pos);
                        self.error(
                            span,
                            "To use a non-default number of inputs, you need to put parentheses around this.",
                        );
                    }
                    break;
                }
                let t = syntax.right.get(goal).copied().unwrap_or(m.wildcard);
                out.push(self.argument(syntax, t)?);
                if goal + 1 < syntax.right.len() {
                    goal += 1;
                }
            }
        } else {
            let n = syntax.right_default(&m);
            for i in 0..n {
                let t = syntax.right[i.min(syntax.right.len().saturating_sub(1))];
                out.push(self.argument(syntax, t)?);
            }
            if syntax.takes_optional_block(&m) && self.kind() == Some(L_BRACKET) {
                let t = *syntax.right.last().unwrap();
                out.push(self.argument(syntax, t)?);
            }
        }
        // Blocks are parsed now that every argument's type is known.
        let goals = self.argument_types(syntax, out.len());
        Ok(out
            .into_iter()
            .zip(goals)
            .map(|(e, g)| Part::Node(self.resolve(e, g)))
            .collect())
    }

    /// The type each of `n` right arguments must have. Arguments before a repeatable one
    /// take the types from the left, arguments after it from the right, and the rest the
    /// repeatable type.
    fn argument_types(&self, syntax: &Syntax, n: usize) -> Vec<u32> {
        let m = self.masks;
        let types = &syntax.right;
        let repeatable = |t: u32| t & m.repeatable != 0;
        let mut out = vec![m.wildcard; n];
        let mut formal = 0;
        let mut actual = 0;
        while formal < types.len() && !repeatable(types[formal]) {
            if actual >= n {
                return out;
            }
            out[actual] = types[formal];
            formal += 1;
            actual += 1;
        }
        if formal < types.len() {
            let mut actual2 = n as isize - 1;
            let mut formal2 = types.len() as isize - 1;
            while formal2 >= 0 && !repeatable(types[formal2 as usize]) {
                if actual2 < actual as isize {
                    break;
                }
                out[actual2 as usize] = types[formal2 as usize];
                formal2 -= 1;
                actual2 -= 1;
            }
            for slot in out
                .iter_mut()
                .take((actual2 + 1).max(0) as usize)
                .skip(actual)
            {
                *slot = types[formal];
            }
        }
        out
    }

    /// One argument expression, parsed at the precedence of its command or reporter. A block
    /// stays unparsed until all the arguments are known.
    fn argument(&mut self, syntax: &Syntax, goal: u32) -> Result<Expr, Fail> {
        let start = self.pos;
        match self.expression(false, syntax.precedence, goal) {
            Ok(e) => Ok(e),
            Err(Fail::Unexpected(p) | Fail::MissingPrefix(p)) => {
                let message = match self.kind_at(p) {
                    Some(R_PAREN) => "Unexpected closing parenthesis.".to_owned(),
                    Some(R_BRACKET) => "Unexpected closing bracket.".to_owned(),
                    Some(IDENT) if matches!(self.class(p), Class::Unknown) => format!(
                        "Nothing named {} has been defined.",
                        self.text(p).to_ascii_uppercase()
                    ),
                    _ => "Missing input.".to_owned(),
                };
                let at = if p < self.limit {
                    p
                } else {
                    start.saturating_sub(1)
                };
                let span = if at < self.sig.len() {
                    self.span(at)
                } else {
                    0..0
                };
                self.error(span, message);
                Err(Fail::Reported)
            }
            Err(Fail::Reported) => Err(Fail::Reported),
        }
    }

    // ------------------------------------------------------------ expressions

    fn expression(&mut self, variadic: bool, precedence: i32, goal: u32) -> Result<Expr, Fail> {
        let m = self.masks;
        let Some(kind) = self.kind() else {
            return Err(Fail::Unexpected(self.pos));
        };
        let want_any = goal == (m.reporter | m.command);
        let want_reporter = want_any || goal == m.reporter;
        let want_command = want_any || goal == m.command;
        let expr = match kind {
            L_PAREN => {
                let open = self.pos;
                self.pos += 1;
                let inner = self.expression(true, MIN_PRECEDENCE, goal)?;
                let inner = self.resolve(inner, goal);
                if self.is_command(self.pos) {
                    let span = self.span(open);
                    self.error(span, "No closing parenthesis for this open parenthesis.");
                    return Err(Fail::Reported);
                }
                Expr::Node(self.close_paren(open, inner)?)
            }
            L_BRACKET => Expr::Delayed(self.delay_block()),
            NUMBER | STRING | EXTENSION_LITERAL | IDENT => {
                let pos = self.pos;
                let class = self.class(pos);
                if kind == IDENT
                    && goal & m.symbol != 0
                    && matches!(
                        class,
                        Class::Command(..) | Class::Reporter(..) | Class::Unknown
                    )
                {
                    self.pos += 1;
                    let part = self.tok_part(pos);
                    return Ok(Expr::Node(self.node(SYMBOL, vec![part])));
                }
                match class {
                    Class::Literal => {
                        if kind == IDENT {
                            self.reference(pos, Target::Constant);
                        }
                        self.pos += 1;
                        let part = self.tok_part(pos);
                        let n = self.node(LITERAL, vec![part]);
                        // A constant is a reporter with no arguments; as the only content of an
                        // expected anonymous procedure it is the concise form.
                        if want_any && !variadic {
                            Expr::Node(self.node(CONCISE, vec![Part::Node(n)]))
                        } else {
                            Expr::Node(n)
                        }
                    }
                    Class::Unknown => {
                        let upper = self.text(pos).to_ascii_uppercase();
                        let found = self.local(&upper);
                        match found {
                            Some(definition) => {
                                self.reference(
                                    pos,
                                    Target::Local {
                                        name: upper,
                                        definition,
                                    },
                                );
                                self.pos += 1;
                                let part = self.tok_part(pos);
                                Expr::Node(self.node(REPORTER_APP, vec![part]))
                            }
                            None => return Err(Fail::Unexpected(pos)),
                        }
                    }
                    Class::Keyword => return Err(Fail::Unexpected(pos)),
                    Class::Reporter(syntax, target) => {
                        let unary = syntax.is_infix() && !want_reporter;
                        if unary && !(variadic && target == Target::Primitive("-".into())) {
                            return Err(Fail::MissingPrefix(pos));
                        }
                        self.reference(pos, target);
                        self.pos += 1;
                        let syntax = if unary {
                            // `(- x)`: negation, allowed only first in parentheses.
                            Syntax {
                                left: None,
                                right: vec![m.number],
                                default_args: 1,
                                minimum_args: 1,
                                precedence: m.normal_precedence,
                                right_associative: false,
                                can_be_concise: false,
                            }
                        } else {
                            syntax
                        };
                        let head = self.tok_part(pos);
                        if want_reporter
                            && !variadic
                            && !syntax.wants_symbolic(&m)
                            && (want_any || syntax.total_default(&m) > 0)
                        {
                            Expr::Node(self.node(CONCISE, vec![head]))
                        } else {
                            let mut parts = vec![head];
                            parts.extend(self.arguments(&syntax, variadic)?);
                            Expr::Node(self.node(REPORTER_APP, parts))
                        }
                    }
                    Class::Command(syntax, target) if want_command => {
                        if !syntax.can_be_concise {
                            return Err(Fail::Unexpected(pos));
                        }
                        self.reference(pos, target);
                        self.pos += 1;
                        let head = self.tok_part(pos);
                        Expr::Node(self.node(CONCISE, vec![head]))
                    }
                    Class::Command(..) => return Err(Fail::Unexpected(pos)),
                }
            }
            _ => return Err(Fail::Unexpected(self.pos)),
        };
        self.more(expr, precedence)
    }

    /// After an expression: infix reporters that bind tighter than `precedence`.
    fn more(&mut self, mut expr: Expr, precedence: i32) -> Result<Expr, Fail> {
        loop {
            if self.kind() != Some(IDENT) {
                return Ok(expr);
            }
            let pos = self.pos;
            let Class::Reporter(syntax, target) = self.class(pos) else {
                return Ok(expr);
            };
            let Some(left) = syntax.left else {
                return Ok(expr);
            };
            if !(syntax.precedence > precedence
                || (syntax.right_associative && syntax.precedence == precedence))
            {
                return Ok(expr);
            }
            self.reference(pos, target);
            self.pos += 1;
            let left = self.resolve(expr, left);
            let mut parts = vec![Part::Node(left), self.tok_part(pos)];
            parts.extend(self.arguments(&syntax, false)?);
            expr = Expr::Node(self.node(REPORTER_APP, parts));
        }
    }

    fn local(&self, upper: &str) -> Option<Range<usize>> {
        self.scope
            .iter()
            .rev()
            .find(|(n, _)| n == upper)
            .map(|(_, s)| s.clone())
    }

    // ------------------------------------------------------------ blocks

    /// The position of the `]` matching the `[` at `open`, or the limit if there is none.
    fn matching_close(&self, open: usize) -> (usize, bool) {
        let mut depth = 0usize;
        let mut i = open;
        while i < self.limit {
            match self.tok(i).kind {
                L_BRACKET => depth += 1,
                R_BRACKET => {
                    depth -= 1;
                    if depth == 0 {
                        return (i, true);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        (self.limit, false)
    }

    /// Skips a bracketed block, to be parsed once its expected type is known.
    fn delay_block(&mut self) -> Delayed {
        let open = self.pos;
        let (close, closed) = self.matching_close(open);
        self.pos = if closed { close + 1 } else { close };
        Delayed {
            open,
            close,
            closed,
            scope: self.scope.clone(),
        }
    }

    /// Turns an expression into a node, parsing a waiting block for the expected type.
    fn resolve(&mut self, e: Expr, goal: u32) -> usize {
        match e {
            Expr::Node(n) => n,
            Expr::Delayed(d) => self.block(d, goal),
        }
    }

    /// The arguments of an anonymous procedure written with `->`, if the block is one:
    /// `[ -> ...]`, `[x -> ...]` or `[[x y] -> ...]`. Returns the position of the arrow.
    fn arrow(&self, d: &Delayed) -> Option<usize> {
        let first = d.open + 1;
        let is_arrow = |p: usize| p < d.close && self.tok(p).kind == IDENT && self.text(p) == "->";
        if is_arrow(first) {
            return Some(first);
        }
        if first < d.close && self.tok(first).kind == IDENT && is_arrow(first + 1) {
            return Some(first + 1);
        }
        if first < d.close && self.tok(first).kind == L_BRACKET {
            let mut p = first + 1;
            while p < d.close && self.tok(p).kind == IDENT {
                p += 1;
            }
            if p < d.close && self.tok(p).kind == R_BRACKET && is_arrow(p + 1) {
                return Some(p + 1);
            }
        }
        None
    }

    /// True if the code starting at `pos` is commands: its first token (after any opening
    /// parentheses) is a command, or there is nothing before `end`.
    fn starts_commands(&mut self, mut pos: usize, end: usize) -> bool {
        while pos < end && self.tok(pos).kind == L_PAREN {
            pos += 1;
        }
        pos >= end || self.tok(pos).kind == R_BRACKET || self.is_command(pos)
    }

    /// Parses a waiting block as what `goal` expects.
    fn block(&mut self, d: Delayed, goal: u32) -> usize {
        let m = self.masks;
        let saved_limit = self.limit;
        let saved_pos = self.pos;
        let saved_scope = std::mem::replace(&mut self.scope, d.scope.clone());
        if !d.closed {
            let span = self.span(d.open);
            self.error(span, "No closing bracket for this open bracket.");
        }
        let compatible = |t: u32| goal & t != 0;
        let list_not_wanted = !compatible(m.list);
        let open = self.tok_part(d.open);
        let close = d.closed.then(|| self.tok_part(d.close));
        let arrow = self.arrow(&d);
        self.limit = d.close;

        let n = if compatible(m.code_block) {
            self.raw_block(&d, CODE_BLOCK)
        } else if let Some(arrow) = arrow {
            // Anonymous procedure with arguments.
            let params = self.lambda_params(d.open + 1, arrow);
            self.pos = arrow + 1;
            let is_command = self.starts_commands(self.pos, d.close);
            let body = if is_command {
                self.command_body()
            } else {
                self.reporter_body(m.wildcard)
            };
            let mut parts = vec![open, Part::Node(params)];
            parts.extend(body);
            parts.extend(close);
            self.node(
                if is_command {
                    ANON_COMMAND
                } else {
                    ANON_REPORTER
                },
                parts,
            )
        } else {
            self.pos = d.open + 1;
            let is_command = self.starts_commands(self.pos, d.close);
            let (kind, body) = if compatible(m.reporter_block) {
                (REPORTER_BLOCK, self.reporter_body(goal))
            } else if compatible(m.command_block) {
                (COMMAND_BLOCK, self.command_body())
            } else if compatible(m.reporter) && !is_command && list_not_wanted {
                (ANON_REPORTER, self.reporter_body(m.wildcard))
            } else if compatible(m.command) && is_command && list_not_wanted {
                (ANON_COMMAND, self.command_body())
            } else if compatible(m.list) {
                (LITERAL_LIST, self.literal_list(&d))
            } else {
                let span = self.span(d.open);
                self.error(span, "Expected a value here, rather than a list or block.");
                (
                    ERROR,
                    (d.open + 1..d.close).map(|i| self.tok_part(i)).collect(),
                )
            };
            let mut parts = vec![open];
            parts.extend(body);
            parts.extend(close);
            self.node(kind, parts)
        };
        self.limit = saved_limit;
        self.pos = saved_pos;
        self.scope = saved_scope;
        n
    }

    /// The tokens of a block as they are, for code blocks.
    fn raw_block(&mut self, d: &Delayed, kind: SyntaxKind) -> usize {
        let mut parts: Vec<Part> = (d.open..d.close).map(|i| self.tok_part(i)).collect();
        if d.closed {
            parts.push(self.tok_part(d.close));
        }
        self.node(kind, parts)
    }

    /// The argument names of an anonymous procedure and its arrow, defining them in scope.
    fn lambda_params(&mut self, from: usize, arrow: usize) -> usize {
        let mut parts = Vec::new();
        self.pos = from;
        while self.pos < arrow {
            match self.tok(self.pos).kind {
                IDENT => {
                    let n = self.define(LocalKind::AnonymousArgument);
                    parts.push(Part::Node(n));
                }
                _ => {
                    parts.push(self.tok_part(self.pos));
                    self.pos += 1;
                }
            }
        }
        parts.push(self.tok_part(arrow));
        self.node(ANON_PARAMS, parts)
    }

    fn command_body(&mut self) -> Vec<Part> {
        self.statements().into_iter().map(Part::Node).collect()
    }

    /// One expression up to the closing bracket.
    fn reporter_body(&mut self, goal: u32) -> Vec<Part> {
        let start = self.pos;
        let mut parts = Vec::new();
        match self.expression(false, MIN_PRECEDENCE, goal) {
            Ok(e) => {
                let n = self.resolve(e, self.masks.wildcard);
                parts.push(Part::Node(n));
            }
            Err(f) => {
                self.report(f, start, "Expected reporter.");
                self.pos = self.limit;
                if start < self.limit {
                    parts.push(Part::Node(self.error_node(start)));
                }
                return parts;
            }
        }
        if !self.at_limit() {
            let span = self.span(self.pos);
            self.error(span, "Expected closing bracket.");
            let from = self.pos;
            self.pos = self.limit;
            parts.push(Part::Node(self.error_node(from)));
        }
        parts
    }

    /// A literal list: numbers, strings, constants and nested lists.
    fn literal_list(&mut self, d: &Delayed) -> Vec<Part> {
        let mut parts = Vec::new();
        let mut p = d.open + 1;
        // Inside braces (agent literals such as `{turtle 3}`) anything goes.
        let mut braces = 0usize;
        while p < d.close {
            let t = self.tok(p).kind;
            match t {
                L_BRACE => braces += 1,
                R_BRACE => braces = braces.saturating_sub(1),
                _ => {}
            }
            let ok = braces > 0
                || match t {
                    NUMBER | STRING | EXTENSION_LITERAL | L_BRACKET | R_BRACKET | L_PAREN
                    | R_PAREN | R_BRACE => true,
                    IDENT => matches!(self.class(p), Class::Literal),
                    _ => false,
                };
            if !ok {
                let span = self.span(p);
                self.error(span, "Expected a literal value.");
            } else if t == IDENT {
                self.reference(p, Target::Constant);
            }
            parts.push(self.tok_part(p));
            p += 1;
        }
        parts
    }

    // ------------------------------------------------------------ writing the tree

    fn first_token(&self, n: usize) -> usize {
        match self.nodes[n].parts.first() {
            Some(Part::Tok(t)) => *t,
            Some(Part::Node(c)) => self.first_token(*c),
            None => usize::MAX,
        }
    }

    /// Writes tokens from the cursor up to (not including) `end`.
    fn emit_until(&self, end: usize, cursor: &mut usize, b: &mut GreenNodeBuilder<'static>) {
        while *cursor < end {
            let t = &self.tokens[*cursor];
            b.token(t.kind.into(), &self.source[t.span.clone()]);
            *cursor += 1;
        }
    }

    fn emit_node(&self, n: usize, cursor: &mut usize, b: &mut GreenNodeBuilder<'static>) {
        let first = self.first_token(n);
        if first != usize::MAX {
            self.emit_until(first, cursor, b);
        }
        b.start_node(self.nodes[n].kind.into());
        for part in &self.nodes[n].parts {
            match *part {
                Part::Tok(t) => {
                    self.emit_until(t, cursor, b);
                    let tok = &self.tokens[t];
                    b.token(tok.kind.into(), &self.source[tok.span.clone()]);
                    *cursor = t + 1;
                }
                Part::Node(c) => self.emit_node(c, cursor, b),
            }
        }
        b.finish_node();
    }
}
