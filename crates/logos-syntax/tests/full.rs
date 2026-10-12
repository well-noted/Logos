//! Part S checks over the corpus, with the primitive table from NetLogo 7.0.4 (needs the
//! bridge):
//!
//! S2: the full parse reports no syntax errors for any corpus model that compiles in NetLogo.
//! S6: the full tree is lossless: its text is the source, for every model.
//!
//! ```text
//! cargo test -p logos-syntax --test full -- --ignored --nocapture
//! ```
//!
//! `LOGOS_S2_FILTER` limits the run to models whose path contains the given text.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use logos_bridge_client::{Bridge, BridgeConfig, find_netlogo_home};
use logos_nlogox::Nlogox;
use logos_syntax::{Context, Language, Structure, parse, parse_with};

const NETLOGO_VERSION: &str = "7.0.4";

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().and_then(|x| x.to_str()) == Some("nlogox") {
            out.push(p);
        }
    }
}

fn models() -> Vec<PathBuf> {
    let mut roots = vec![repo().join("corpus/own")];
    if let Some(home) = find_netlogo_home(NETLOGO_VERSION) {
        roots.push(home.join("models"));
    }
    if let Some(dirs) = std::env::var_os("LOGOS_CORPUS_DIRS") {
        roots.extend(std::env::split_paths(&dirs));
    }
    let mut out = Vec::new();
    for r in roots {
        walk(&r, &mut out);
    }
    out.sort();
    out.dedup();
    if let Ok(f) = std::env::var("LOGOS_S2_FILTER") {
        out.retain(|p| p.to_string_lossy().contains(&f));
    }
    out
}

/// A model's sources: its code and its `__includes` files, and the globals its widgets define.
struct Program {
    code: String,
    includes: Vec<(PathBuf, String)>,
    interface_globals: Vec<String>,
    structure: Structure,
}

fn program(path: &Path) -> Program {
    let f = Nlogox::parse(std::fs::read_to_string(path).unwrap()).unwrap();
    let code = f.code().unwrap();
    let interface_globals = f
        .widgets()
        .into_iter()
        .filter_map(|w| f.attribute_value(w, "variable"))
        .filter(|v| !v.is_empty())
        .map(|v| v.to_ascii_uppercase())
        .collect();
    let mut structure = Structure::default();
    structure.add(&parse(&code).source_file());
    let dir = path.parent().unwrap();
    let mut includes = Vec::new();
    let mut seen = BTreeSet::new();
    let mut i = 0;
    while i < structure.includes.len() {
        let inc = structure.includes[i].clone();
        i += 1;
        if seen.insert(inc.clone())
            && let Ok(bytes) = std::fs::read(dir.join(&inc))
        {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            structure.add(&parse(&text).source_file());
            includes.push((dir.join(&inc), text));
        }
    }
    Program {
        code,
        includes,
        interface_globals,
        structure,
    }
}

#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn s2_s6_full_parse_over_the_corpus() {
    let home = find_netlogo_home(NETLOGO_VERSION).expect("NetLogo 7.0.4");
    let jar = repo().join("bridge/build/libs/logos-bridge-0.0.0.jar");
    let bridge = Bridge::spawn(&BridgeConfig::new(home, NETLOGO_VERSION, jar)).expect("bridge");
    let models = models();
    // One table per set of extensions.
    let tables: Mutex<BTreeMap<Vec<String>, Option<Language>>> = Mutex::new(BTreeMap::new());
    let workers = std::env::var("LOGOS_WORKERS")
        .ok()
        .and_then(|w| w.parse().ok())
        .unwrap_or(4usize);
    let queue = Mutex::new(models.iter());
    let counts = Mutex::new((0usize, 0usize, 0usize)); // parsed, compiled, skipped
    let failures = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let Some(path) = queue.lock().unwrap().next() else {
                        break;
                    };
                    let mut prog = program(path);
                    // NetLogo adds the globals and procedures generated from a system dynamics
                    // diagram to the program; they are not in the code, so take them from
                    // NetLogo.
                    if Nlogox::parse(std::fs::read_to_string(path).unwrap())
                        .unwrap()
                        .section("systemDynamics")
                        .is_some()
                        && let Ok(o) = bridge.open(path)
                    {
                        if let Ok(p) = bridge.program(o.workspace) {
                            for g in p.globals {
                                if !prog.structure.globals.contains(&g) {
                                    prog.structure.globals.push(g);
                                }
                            }
                            for proc in p.procedures {
                                if !prog.structure.procedures.iter().any(|q| q.0 == proc.name) {
                                    prog.structure.procedures.push((
                                        proc.name,
                                        proc.is_reporter,
                                        proc.args,
                                    ));
                                }
                            }
                        }
                        let _ = bridge.dispose(o.workspace);
                    }
                    let mut exts: Vec<String> = prog
                        .structure
                        .extensions
                        .iter()
                        .map(|e| e.to_ascii_lowercase())
                        .collect();
                    exts.sort();
                    exts.dedup();
                    let lang = {
                        let mut t = tables.lock().unwrap();
                        t.entry(exts.clone())
                            .or_insert_with(|| {
                                let names: Vec<&str> = exts.iter().map(String::as_str).collect();
                                bridge
                                    .primitives(&names)
                                    .ok()
                                    .and_then(|r| Language::from_table(&r).ok())
                            })
                            .clone()
                    };
                    let Some(lang) = lang else {
                        counts.lock().unwrap().2 += 1;
                        continue;
                    };
                    let ctx = Context {
                        language: &lang,
                        structure: &prog.structure,
                        interface_globals: &prog.interface_globals,
                    };
                    let mut errors = Vec::new();
                    for (name, src) in std::iter::once((path.clone(), prog.code.clone()))
                        .chain(prog.includes.iter().cloned())
                    {
                        let full = parse_with(&src, &ctx);
                        // S6.
                        assert_eq!(
                            full.parse.syntax().to_string(),
                            src,
                            "tree text differs: {}",
                            name.display()
                        );
                        for e in &full.parse.errors {
                            let line = src[..e.span.start.min(src.len())].lines().count().max(1);
                            let text = &src[e.span.clone()];
                            errors.push(format!(
                                "{}:{line} {} [{}]",
                                name.file_name().unwrap().to_string_lossy(),
                                e.message,
                                text.chars().take(40).collect::<String>()
                            ));
                        }
                    }
                    counts.lock().unwrap().0 += 1;
                    if errors.is_empty() {
                        continue;
                    }
                    // Only models that compile in NetLogo count for S2.
                    let compiled = match bridge.open(path) {
                        Ok(o) => {
                            let _ = bridge.dispose(o.workspace);
                            o.compiled
                        }
                        Err(_) => false,
                    };
                    if compiled {
                        counts.lock().unwrap().1 += 1;
                        failures.lock().unwrap().push((path.clone(), errors));
                    }
                }
            });
        }
    });
    bridge.shutdown().unwrap();
    let (parsed, _, skipped) = counts.into_inner().unwrap();
    let mut failures = failures.into_inner().unwrap();
    failures.sort();
    // Group the first error of each model by message, to see the causes.
    let mut by_message: BTreeMap<String, usize> = BTreeMap::new();
    for (_, e) in &failures {
        let m = e[0].split_once(' ').map(|(_, m)| m).unwrap_or(&e[0]);
        let m = m.split(" [").next().unwrap_or(m).to_owned();
        *by_message.entry(m).or_default() += 1;
    }
    eprintln!(
        "S2/S6: {} models, {parsed} parsed (trees lossless), {skipped} without a primitive table; \
         {} compile in NetLogo but have parse errors",
        models.len(),
        failures.len()
    );
    for (m, n) in &by_message {
        eprintln!("  {n:>4}  {m}");
    }
    for (p, e) in failures.iter().take(25) {
        eprintln!("  {}: {}", p.file_name().unwrap().to_string_lossy(), e[0]);
    }
    assert!(failures.is_empty());
}

/// The tree of a procedure body as nested lists: nodes as `(KIND ...)`, tokens as their text,
/// trivia left out.
fn shape(node: &logos_syntax::SyntaxNode) -> String {
    let mut out = format!("({:?}", node.kind());
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(n) => {
                out.push(' ');
                out.push_str(&shape(&n));
            }
            rowan::NodeOrToken::Token(t) if !t.kind().is_trivia() => {
                out.push(' ');
                out.push_str(t.text());
            }
            _ => {}
        }
    }
    out.push(')');
    out
}

/// The shape of the statements in the first procedure's body.
fn body_shape(code: &str, lang: &Language) -> (String, logos_syntax::FullParse) {
    let structure = logos_syntax::structure_of([code]);
    let globals = vec!["SPEED".to_owned()];
    let ctx = Context {
        language: lang,
        structure: &structure,
        interface_globals: &globals,
    };
    let full = parse_with(code, &ctx);
    let body = full
        .parse
        .source_file()
        .procedures()
        .next()
        .unwrap()
        .body()
        .unwrap();
    let inner: Vec<String> = body.children().map(|n| shape(&n)).collect();
    (inner.join(" "), full)
}

/// Statements and expressions parse into the expected trees, and names resolve to what
/// NetLogo resolves them to.
#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn s_expressions_parse_as_netlogo_reads_them() {
    use logos_syntax::{Owner, Target};
    let home = find_netlogo_home(NETLOGO_VERSION).expect("NetLogo 7.0.4");
    let jar = repo().join("bridge/build/libs/logos-bridge-0.0.0.jar");
    let bridge = Bridge::spawn(&BridgeConfig::new(home, NETLOGO_VERSION, jar)).expect("bridge");
    let lang = Language::from_table(&bridge.primitives(&[]).unwrap()).unwrap();
    bridge.shutdown().unwrap();
    let decls = "globals [g]\nbreed [frogs frog]\nfrogs-own [age]\n";
    let cases: &[(&str, &str)] = &[
        // Arguments take the command's precedence; `+` binds looser than a prefix reporter.
        (
            "to go fd random 10 + 1 end",
            "(STATEMENT fd (REPORTER_APP (REPORTER_APP random (LITERAL 10)) + (LITERAL 1)))",
        ),
        // `with` and `of` bind tighter than a prefix reporter's argument.
        (
            "to go show count frogs with [ age > 2 ] end",
            "(STATEMENT show (REPORTER_APP count (REPORTER_APP (REPORTER_APP frogs) with \
             (REPORTER_BLOCK [ (REPORTER_APP (REPORTER_APP age) > (LITERAL 2)) ]))))",
        ),
        (
            "to go show [ age ] of frog 0 end",
            "(STATEMENT show (REPORTER_APP (REPORTER_BLOCK [ (REPORTER_APP age) ]) of \
             (REPORTER_APP frog (LITERAL 0))))",
        ),
        // Command blocks, optional blocks, let scope.
        (
            "to go create-frogs 2 [ let d 3 fd d ] ask frogs [ set age age + 1 ] end",
            "(STATEMENT create-frogs (LITERAL 2) (COMMAND_BLOCK [ (STATEMENT let (NAME d) \
             (LITERAL 3)) (STATEMENT fd (REPORTER_APP d)) ])) (STATEMENT ask \
             (REPORTER_APP frogs) (COMMAND_BLOCK [ (STATEMENT set (REPORTER_APP age) \
             (REPORTER_APP (REPORTER_APP age) + (LITERAL 1))) ]))",
        ),
        // Anonymous procedures, concise syntax, variadic calls in parentheses.
        (
            "to go show map [ x -> x * 2 ] [ 1 2 ] foreach [ 1 2 ] print \
             show (list 1 2 3) (foreach [1] [2] [ [a b] -> show a + b ]) end",
            "(STATEMENT show (REPORTER_APP map (ANON_REPORTER [ (ANON_PARAMS (NAME x) ->) \
             (REPORTER_APP (REPORTER_APP x) * (LITERAL 2)) ]) (LITERAL_LIST [ 1 2 ]))) \
             (STATEMENT foreach (LITERAL_LIST [ 1 2 ]) (CONCISE print)) (STATEMENT show \
             (PARENS ( (REPORTER_APP list (LITERAL 1) (LITERAL 2) (LITERAL 3)) ))) (PARENS ( \
             (STATEMENT foreach (LITERAL_LIST [ 1 ]) (LITERAL_LIST [ 2 ]) (ANON_COMMAND [ \
             (ANON_PARAMS [ (NAME a) (NAME b) ] ->) (STATEMENT show (REPORTER_APP \
             (REPORTER_APP a) + (REPORTER_APP b))) ])) ))",
        ),
        // Unary minus first in parentheses; constants; interface globals; multi-let.
        (
            "to go show (- g) set speed red let [p q] [1 2] end",
            "(STATEMENT show (PARENS ( (REPORTER_APP - (REPORTER_APP g)) ))) (STATEMENT set \
             (REPORTER_APP speed) (LITERAL red)) (STATEMENT let (NAMES [ (NAME p) (NAME q) ]) \
             (LITERAL_LIST [ 1 2 ]))",
        ),
    ];
    let mut failed = 0;
    for (proc, expected) in cases {
        let code = format!("{decls}{proc}");
        let (got, full) = body_shape(&code, &lang);
        let expected = expected.split_whitespace().collect::<Vec<_>>().join(" ");
        if std::env::var("LOGOS_SHOW").is_ok() {
            eprintln!("{got}");
        }
        if got != expected || !full.parse.errors.is_empty() {
            failed += 1;
            eprintln!(
                "{proc}\n  expected {expected}\n  got      {got}\n  errors   {:?}",
                full.parse.errors
            );
        }
    }
    // References: the breed variable, the breed primitive, the let variable.
    let code = format!("{decls}to go ask frogs [ let d age show d ] end");
    let (_, full) = body_shape(&code, &lang);
    let targets: Vec<&Target> = full.references.iter().map(|r| &r.target).collect();
    assert!(targets.contains(&&Target::AgentVariable {
        owner: Owner::Breed("FROGS".into()),
        name: "AGE".into()
    }));
    assert!(targets.contains(&&Target::BreedPrimitive {
        pattern: "<breeds>".into(),
        breed: "FROGS".into()
    }));
    let d_def = full
        .locals
        .iter()
        .find(|l| l.name == "D")
        .unwrap()
        .span
        .clone();
    assert!(targets.contains(&&Target::Local {
        name: "D".into(),
        definition: d_def
    }));
    assert_eq!(failed, 0, "{failed} cases differ");
}
