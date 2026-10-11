//! Spike B lexer and parser tests over the corpus (docs/phase-0-spec.md, B4, B5, B7).
//!
//! The corpus is the same as `logos-nlogox`'s: AA4-Combined, the NetLogo 7.0.4 Models Library
//! when NetLogo is installed, and every `.nlogox` under the folders in `LOGOS_CORPUS_DIRS`.
//! `.nls` files next to those models are included too.
//!
//! ```text
//! cargo test -p logos-syntax --test corpus -- --nocapture
//! cargo test -p logos-syntax --test corpus --release -- --nocapture b7   # timing
//! cargo test -p logos-syntax --test corpus -- --ignored --nocapture     # B5, needs the bridge
//! ```

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use logos_nlogox::Nlogox;
use logos_syntax::{Structure, lex, parse};

const NETLOGO_VERSION: &str = "7.0.4";

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, ext, out);
        } else if p.extension().and_then(|x| x.to_str()) == Some(ext) {
            out.push(p);
        }
    }
}

fn roots() -> Vec<PathBuf> {
    let mut roots = vec![repo().join("corpus/own")];
    if let Some(home) = logos_bridge_client::find_netlogo_home(NETLOGO_VERSION) {
        roots.push(home.join("models"));
    }
    if let Some(dirs) = std::env::var_os("LOGOS_CORPUS_DIRS") {
        roots.extend(std::env::split_paths(&dirs));
    }
    roots
}

/// A source file and its NetLogo code.
type Source = (PathBuf, String);

/// (path, code) for every corpus model, and every `.nls` file.
fn sources() -> (Vec<Source>, Vec<Source>) {
    let mut models = Vec::new();
    let mut includes = Vec::new();
    for root in roots() {
        walk(&root, "nlogox", &mut models);
        walk(&root, "nls", &mut includes);
    }
    models.sort();
    models.dedup();
    includes.sort();
    includes.dedup();
    let models = models
        .into_iter()
        .map(|p| {
            let f = Nlogox::parse(std::fs::read_to_string(&p).unwrap()).unwrap();
            let code = f.code().unwrap();
            (p, code)
        })
        .collect();
    let includes = includes
        .into_iter()
        .map(|p| {
            let text = String::from_utf8_lossy(&std::fs::read(&p).unwrap()).into_owned();
            (p, text)
        })
        .collect();
    (models, includes)
}

/// B4: joining the tokens reproduces the code exactly; the parse tree's text is the code.
#[test]
fn b4_lexer_and_tree_are_lossless() {
    let (models, includes) = sources();
    let mut bytes = 0;
    let mut tokens = 0;
    let mut with_errors = Vec::new();
    for (path, code) in models.iter().chain(&includes) {
        let (t, _) = lex(code);
        let joined: String = t.iter().map(|t| &code[t.span.clone()]).collect();
        assert_eq!(&joined, code, "{}", path.display());
        let p = parse(code);
        assert_eq!(&p.syntax().to_string(), code, "{}", path.display());
        bytes += code.len();
        tokens += t.len();
        if !p.errors.is_empty() {
            with_errors.push((path.clone(), p.errors.clone()));
        }
    }
    eprintln!(
        "B4: {} models and {} .nls files, {bytes} bytes, {tokens} tokens: joining tokens \
         reproduces every source; {} sources have syntax errors",
        models.len(),
        includes.len(),
        with_errors.len()
    );
    for (p, e) in with_errors.iter().take(20) {
        eprintln!(
            "  {}: {:?}",
            p.file_name().unwrap().to_string_lossy(),
            e.iter().take(3).map(|e| &e.message).collect::<Vec<_>>()
        );
    }
}

/// B7: lexing and parsing the largest model's code. The 10 ms target applies to a release
/// build; a debug build only prints the time.
#[test]
fn b7_largest_model_parses_quickly() {
    let (models, includes) = sources();
    let (path, code) = models
        .iter()
        .chain(&includes)
        .max_by_key(|(_, c)| c.len())
        .unwrap();
    // Warm up, then take the best of several runs.
    let _ = parse(code);
    let mut best = f64::MAX;
    for _ in 0..20 {
        let t = Instant::now();
        let p = parse(code);
        std::hint::black_box(&p);
        best = best.min(t.elapsed().as_secs_f64() * 1000.0);
    }
    let release = !cfg!(debug_assertions);
    eprintln!(
        "B7: largest source {} ({} bytes, {} lines): lex + parse {best:.2} ms ({} build)",
        path.file_name().unwrap().to_string_lossy(),
        code.len(),
        code.lines().count(),
        if release { "release" } else { "debug" }
    );
    if release {
        assert!(best < 10.0, "{best:.2} ms");
    }
}

/// The structure from a model's code plus its `__includes` files (paths relative to the
/// model's folder).
fn structure(path: &Path, code: &str) -> Structure {
    let mut s = Structure::default();
    s.add(&parse(code).source_file());
    let dir = path.parent().unwrap();
    let mut seen = BTreeSet::new();
    let mut i = 0;
    while i < s.includes.len() {
        let inc = s.includes[i].clone();
        i += 1;
        if seen.insert(inc.clone())
            && let Ok(bytes) = std::fs::read(dir.join(&inc))
        {
            s.add(&parse(&String::from_utf8_lossy(&bytes)).source_file());
        }
    }
    s
}

/// B5: the parser's declarations and procedures match NetLogo's for every corpus model that
/// compiles.
#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn b5_structure_matches_netlogo() {
    use logos_bridge_client::{Bridge, BridgeConfig, find_netlogo_home};
    use std::sync::Mutex;

    let (models, _) = sources();
    // NetLogo adds the code it generates from a system dynamics diagram to the model's
    // program; that code is not in the code section, so those models are not compared.
    let (sd, models): (Vec<_>, Vec<_>) = models.into_iter().partition(|(p, _)| {
        Nlogox::parse(std::fs::read_to_string(p).unwrap())
            .unwrap()
            .section("systemDynamics")
            .is_some()
    });
    let home = find_netlogo_home(NETLOGO_VERSION).expect("NetLogo 7.0.4");
    let jar = repo().join("bridge/build/libs/logos-bridge-0.0.0.jar");
    let bridge = Bridge::spawn(&BridgeConfig::new(home, NETLOGO_VERSION, jar)).expect("bridge");
    let workers = std::env::var("LOGOS_WORKERS")
        .ok()
        .and_then(|w| w.parse().ok())
        .unwrap_or(4usize);
    let queue = Mutex::new(models.iter());
    let compared = Mutex::new(0usize);
    let skipped = Mutex::new(0usize);
    let failures = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let Some((path, code)) = queue.lock().unwrap().next() else {
                        break;
                    };
                    let Ok(open) = bridge.open(path) else {
                        *skipped.lock().unwrap() += 1;
                        continue;
                    };
                    if !open.compiled {
                        let _ = bridge.dispose(open.workspace);
                        *skipped.lock().unwrap() += 1;
                        continue;
                    }
                    let nl = bridge.program(open.workspace).unwrap();
                    let _ = bridge.dispose(open.workspace);
                    let ours = structure(path, code);
                    let mut diffs = Vec::new();
                    let mut check =
                        |what: &str, a: &dyn std::fmt::Debug, b: &dyn std::fmt::Debug| {
                            let (a, b) = (format!("{a:?}"), format!("{b:?}"));
                            if a != b {
                                diffs.push(format!("{what}: ours {a}, NetLogo {b}"));
                            }
                        };
                    check("globals", &ours.globals, &nl.globals);
                    check("turtles-own", &ours.turtles_own, &nl.turtles_own);
                    check("patches-own", &ours.patches_own, &nl.patches_own);
                    check("links-own", &ours.links_own, &nl.links_own);
                    let our_breeds: Vec<_> = ours
                        .breeds
                        .iter()
                        .map(|b| {
                            (
                                b.name.clone(),
                                b.singular.clone(),
                                b.is_link,
                                b.directed,
                                b.owns.clone(),
                            )
                        })
                        .collect();
                    let nl_breeds: Vec<_> = nl
                        .breeds
                        .iter()
                        .map(|b| {
                            (
                                b.name.clone(),
                                Some(b.singular.clone()),
                                b.is_link_breed,
                                b.is_directed,
                                b.owns.clone(),
                            )
                        })
                        .collect();
                    check("breeds", &our_breeds, &nl_breeds);
                    let our_procs: BTreeSet<_> = ours.procedures.iter().cloned().collect();
                    let nl_procs: BTreeSet<_> = nl
                        .procedures
                        .iter()
                        .map(|p| (p.name.clone(), p.is_reporter, p.args.clone()))
                        .collect();
                    check("procedures", &our_procs, &nl_procs);
                    *compared.lock().unwrap() += 1;
                    if !diffs.is_empty() {
                        failures.lock().unwrap().push(format!(
                            "{}: {}",
                            path.display(),
                            diffs.join("; ")
                        ));
                    }
                }
            });
        }
    });
    bridge.shutdown().unwrap();
    let failures = failures.into_inner().unwrap();
    eprintln!(
        "B5: {} models; {} compile and were compared, {} skipped (don't compile or don't open), \
         {} with system dynamics not compared; {} differ",
        models.len() + sd.len(),
        compared.into_inner().unwrap(),
        skipped.into_inner().unwrap(),
        sd.len(),
        failures.len()
    );
    for f in failures.iter().take(20) {
        eprintln!("  {}", f.chars().take(600).collect::<String>());
    }
    assert!(failures.is_empty());
}
