//! What the expression parser needs to know about NetLogo: the syntax of every primitive, the
//! built-in agent variables, constants, keywords and breed primitives. All of it comes from the
//! user's installed NetLogo through the bridge (`engine.primitives`, D-01); none of it is
//! written into Logos.

use std::collections::{HashMap, HashSet};

use logos_protocol::{BreedKind, PrimitiveInfo, PrimitiveKind, PrimitivesResult};

/// The masks and precedences the parser uses, as NetLogo's `Syntax` defines them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Masks {
    pub number: u32,
    pub list: u32,
    pub command: u32,
    pub reporter: u32,
    pub reference: u32,
    pub command_block: u32,
    pub reporter_block: u32,
    pub repeatable: u32,
    pub optional: u32,
    pub code_block: u32,
    pub symbol: u32,
    pub wildcard: u32,
    pub command_precedence: i32,
    pub normal_precedence: i32,
}

/// How a command or reporter takes its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Syntax {
    /// The left argument's type, for infix reporters.
    pub left: Option<u32>,
    /// The types of the arguments on the right.
    pub right: Vec<u32>,
    /// NetLogo's default argument count; includes an optional command block.
    pub default_args: usize,
    pub minimum_args: usize,
    pub precedence: i32,
    pub right_associative: bool,
    /// Usable by name alone where an anonymous procedure is expected.
    pub can_be_concise: bool,
}

impl Syntax {
    fn from_info(p: &PrimitiveInfo) -> Syntax {
        Syntax {
            left: p.left.as_ref().map(|t| t.mask),
            right: p.args.iter().map(|t| t.mask).collect(),
            default_args: p.default_arg_count as usize,
            minimum_args: p.minimum_arg_count as usize,
            precedence: p.precedence,
            right_associative: p.is_right_associative,
            can_be_concise: p.can_be_concise,
        }
    }

    /// A user procedure (or a reporter used as a value) with `args` arguments of any type.
    pub fn procedure(args: usize, masks: &Masks, command: bool) -> Syntax {
        Syntax {
            left: None,
            right: vec![masks.wildcard; args],
            default_args: args,
            minimum_args: args,
            precedence: if command {
                masks.command_precedence
            } else {
                masks.normal_precedence
            },
            right_associative: false,
            can_be_concise: true,
        }
    }

    pub fn is_infix(&self) -> bool {
        self.left.is_some()
    }

    pub fn is_variadic(&self, m: &Masks) -> bool {
        self.right.iter().any(|t| t & m.repeatable != 0)
    }

    pub fn takes_optional_block(&self, m: &Masks) -> bool {
        self.right.last().is_some_and(|t| t & m.optional != 0)
    }

    /// Arguments on the right taken without parentheses, not counting an optional block.
    pub fn right_default(&self, m: &Masks) -> usize {
        if self.takes_optional_block(m) {
            self.default_args.saturating_sub(1)
        } else {
            self.default_args
        }
    }

    pub fn total_default(&self, m: &Masks) -> usize {
        self.right_default(m) + usize::from(self.is_infix())
    }

    /// True if any argument is a symbol or a code block (such reporters are never concise).
    pub fn wants_symbolic(&self, m: &Masks) -> bool {
        let symbolic = m.symbol | m.code_block;
        self.left.is_some_and(|l| l & symbolic != 0) || self.right.iter().any(|t| t & symbolic != 0)
    }
}

/// A primitive NetLogo derives from a declared breed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreedPrimitive {
    /// For example `create-<breeds>` or `<breed>-neighbors`.
    pub pattern: String,
    pub breed_kind: BreedKind,
    pub command: bool,
    pub syntax: Syntax,
}

/// The tables from `engine.primitives`, indexed for the parser.
#[derive(Debug, Clone)]
pub struct Language {
    pub masks: Masks,
    pub commands: HashMap<String, Syntax>,
    pub reporters: HashMap<String, Syntax>,
    pub extension_commands: HashMap<String, Syntax>,
    pub extension_reporters: HashMap<String, Syntax>,
    pub constants: HashSet<String>,
    pub keywords: HashSet<String>,
    pub turtle_variables: Vec<String>,
    pub patch_variables: Vec<String>,
    pub link_variables: Vec<String>,
    pub observer_variables: Vec<String>,
    pub breed_primitives: Vec<BreedPrimitive>,
}

impl Language {
    /// Builds the tables from the bridge's answer. Names are lower case.
    pub fn from_table(t: &PrimitivesResult) -> Result<Language, String> {
        let mask = |name: &str| -> Result<i64, String> {
            t.type_masks
                .get(name)
                .copied()
                .ok_or_else(|| format!("the primitive table has no type mask {name}"))
        };
        let masks = Masks {
            number: mask("number")? as u32,
            list: mask("list")? as u32,
            command: mask("command")? as u32,
            reporter: mask("reporter")? as u32,
            reference: mask("reference")? as u32,
            command_block: mask("commandBlock")? as u32,
            reporter_block: mask("reporterBlock")? as u32,
            repeatable: mask("repeatable")? as u32,
            optional: mask("optional")? as u32,
            code_block: mask("codeBlock")? as u32,
            symbol: mask("symbol")? as u32,
            wildcard: mask("wildcard")? as u32,
            command_precedence: mask("commandPrecedence")? as i32,
            normal_precedence: mask("normalPrecedence")? as i32,
        };
        let mut lang = Language {
            masks,
            commands: HashMap::new(),
            reporters: HashMap::new(),
            extension_commands: HashMap::new(),
            extension_reporters: HashMap::new(),
            constants: t.constants.iter().map(|c| c.name.clone()).collect(),
            keywords: t.keywords.iter().cloned().collect(),
            turtle_variables: names(&t.agent_variables.turtle),
            patch_variables: names(&t.agent_variables.patch),
            link_variables: names(&t.agent_variables.link),
            observer_variables: names(&t.agent_variables.observer),
            breed_primitives: Vec::new(),
        };
        for p in &t.primitives {
            let table = match (p.kind, p.extension.is_some()) {
                (PrimitiveKind::Command, false) => &mut lang.commands,
                (PrimitiveKind::Reporter, false) => &mut lang.reporters,
                (PrimitiveKind::Command, true) => &mut lang.extension_commands,
                (PrimitiveKind::Reporter, true) => &mut lang.extension_reporters,
            };
            table.insert(p.name.clone(), Syntax::from_info(p));
        }
        for b in &t.breed_primitives {
            lang.breed_primitives.push(BreedPrimitive {
                pattern: b.primitive.name.clone(),
                breed_kind: b.breed_kind,
                command: b.primitive.kind == PrimitiveKind::Command,
                syntax: Syntax::from_info(&b.primitive),
            });
        }
        if lang.commands.is_empty() || lang.keywords.is_empty() {
            return Err("the primitive table is incomplete".into());
        }
        Ok(lang)
    }

    /// True for `to`, `end`, `globals`, ... and any name ending in `-own`.
    pub fn is_keyword(&self, lower: &str) -> bool {
        self.keywords.contains(lower) || (lower.ends_with("-own") && lower.len() > 4)
    }
}

fn names(vars: &[logos_protocol::VariableInfo]) -> Vec<String> {
    vars.iter().map(|v| v.name.clone()).collect()
}
