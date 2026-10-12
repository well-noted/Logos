//! S3: renaming is safe. Every procedure and every global declared in the code of AA4 and of
//! a sample of 50 corpus models is renamed with the language server's rename; the renamed
//! model must compile in NetLogo, and its structure (`workspace.program`, as in B5) must be
//! the original's with only that name changed. Uses in widgets and BehaviorSpace experiments
//! are renamed too. Needs NetLogo 7.0.4 and the bridge jar:
//!
//! ```text
//! cargo test -p logos-lsp --release --test s3 -- --ignored --nocapture
//! ```
//!
//! `LOGOS_WORKERS` sets how many models are checked at once (default 4).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use logos_bridge_client::{Bridge, BridgeConfig, find_netlogo_home};
use logos_lsp::analysis::Key;
use logos_lsp::program::{FileKey, Program, normalize};
use logos_nlogox::Nlogox;
use logos_protocol::ProgramResult;
use logos_syntax::Language;

const NETLOGO_VERSION: &str = "7.0.4";
const SAMPLE: usize = 50;

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

/// AA4 first, then the NetLogo models library and any `LOGOS_CORPUS_DIRS`, in path order.
fn candidates(home: &Path) -> (PathBuf, Vec<PathBuf>) {
    let aa4 = repo().join("corpus/own/aa4-combined/AA4-Combined.nlogox");
    let mut others = Vec::new();
    walk(&home.join("models"), &mut others);
    if let Some(dirs) = std::env::var_os("LOGOS_CORPUS_DIRS") {
        for d in std::env::split_paths(&dirs) {
            walk(&d, &mut others);
        }
    }
    others.sort();
    others.dedup();
    others.retain(|p| *p != aa4);
    (aa4, others)
}

/// A model and its include files copied into a folder of their own, so they can be edited.
struct Copy {
    dir: PathBuf,
    model: PathBuf,
    /// (path in the copy, original text)
    files: Vec<(PathBuf, String)>,
}

impl Copy {
    /// Copies the model and the include files its code names. `None` if an include file is
    /// outside the model's folder or the model has a system dynamics diagram (NetLogo adds
    /// code generated from it, which can use the names being renamed).
    fn new(model: &Path, n: usize) -> Option<Copy> {
        let text = std::fs::read_to_string(model).ok()?;
        let f = Nlogox::parse(text.clone()).ok()?;
        if f.section("systemDynamics").is_some() {
            return None;
        }
        let code = f.code().ok()?;
        let includes = logos_syntax::structure_of([code.as_str()]).includes;
        let dir = std::env::temp_dir().join(format!("logos-s3-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok()?;
        let src_dir = model.parent()?;
        let copy = dir.join(model.file_name()?);
        let mut files = vec![(copy.clone(), text)];
        for inc in includes {
            let rel = Path::new(&inc);
            if rel.is_absolute()
                || rel
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return None;
            }
            let t = std::fs::read(src_dir.join(rel)).ok()?;
            let target = dir.join(rel);
            std::fs::create_dir_all(target.parent()?).ok()?;
            files.push((target, String::from_utf8_lossy(&t).into_owned()));
        }
        let c = Copy {
            dir,
            model: copy,
            files,
        };
        c.restore();
        Some(c)
    }

    fn restore(&self) {
        for (p, t) in &self.files {
            std::fs::write(p, t).unwrap();
        }
    }
}

impl Drop for Copy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// `workspace.program` with one name replaced, for comparing.
fn renamed(p: &ProgramResult, old: &str, new: &str) -> String {
    let swap = |s: &String| if s == old { new.to_owned() } else { s.clone() };
    let mut p = p.clone();
    p.globals = p.globals.iter().map(swap).collect();
    for proc in &mut p.procedures {
        proc.name = swap(&proc.name);
    }
    p.procedures.sort_by(|a, b| a.name.cmp(&b.name));
    format!("{p:?}")
}

fn sorted(p: &ProgramResult) -> String {
    let mut p = p.clone();
    p.procedures.sort_by(|a, b| a.name.cmp(&b.name));
    format!("{p:?}")
}

struct Outcome {
    model: PathBuf,
    names: usize,
    failures: Vec<String>,
    /// Edits made outside the code section (widgets, experiments).
    outside: usize,
}

/// Renames each procedure and global of the model in `copy`. `None` if the original doesn't
/// open and compile from the copy.
fn check_model(
    bridge: &Bridge,
    language: &dyn Fn(&[String]) -> Option<Arc<Language>>,
    copy: &Copy,
    original: &Path,
) -> Option<Outcome> {
    let open = bridge.open(&copy.model).ok()?;
    if !open.compiled {
        let _ = bridge.dispose(open.workspace);
        return None;
    }
    let base = bridge.program(open.workspace).ok()?;
    let _ = bridge.dispose(open.workspace);
    let read = |k: &FileKey| std::fs::read_to_string(k.path()?).ok();
    let program = Program::build(FileKey::Path(normalize(&copy.model)), &read, language);
    let s = &program.analysis.structure;
    let mut keys: Vec<Key> = s
        .procedures
        .iter()
        .map(|p| Key::Procedure(p.0.clone()))
        .chain(s.globals.iter().map(|g| Key::Global(g.clone())))
        .collect();
    keys.dedup();
    let mut failures = Vec::new();
    let mut outside = 0;
    for (k, key) in keys.iter().enumerate() {
        let (Key::Procedure(old) | Key::Global(old)) = key else {
            continue;
        };
        let new = format!("logos-renamed-{k}");
        let fail = |why: String| format!("{}: {why}", old.to_ascii_lowercase());
        let Some(def) = program.analysis.definitions(key).into_iter().next() else {
            failures.push(fail("no definition found".into()));
            continue;
        };
        let locs = match program.analysis.rename(def.source, def.range.start, &new) {
            Ok(l) => l,
            Err(e) => {
                failures.push(fail(format!("refused: {e}")));
                continue;
            }
        };
        // Apply the edits to each file, from the end so earlier offsets stay valid.
        let mut texts: HashMap<usize, String> = HashMap::new();
        let mut by_member: HashMap<usize, Vec<std::ops::Range<usize>>> = HashMap::new();
        for l in &locs {
            let m = &program.members[l.source];
            by_member
                .entry(l.source)
                .or_default()
                .push(m.view.range_to_doc(&l.range));
        }
        // Uses in widgets and experiments, already in model file offsets.
        let in_widgets = program.uses_outside_code(key);
        outside += in_widgets.len();
        by_member.entry(0).or_default().extend(in_widgets);
        for (i, mut ranges) in by_member {
            ranges.sort_by_key(|r| std::cmp::Reverse(r.start));
            let mut t = program.members[i].text.clone();
            for r in ranges {
                t.replace_range(r, &new);
            }
            texts.insert(i, t);
        }
        for (i, t) in &texts {
            std::fs::write(program.members[*i].key.path().unwrap(), t).unwrap();
        }
        let result = bridge.open(&copy.model);
        copy.restore();
        let open = match result {
            Ok(o) => o,
            Err(e) => {
                failures.push(fail(format!(
                    "NetLogo couldn't open the renamed model: {e}"
                )));
                continue;
            }
        };
        if !open.compiled {
            let _ = bridge.dispose(open.workspace);
            failures.push(fail(format!(
                "the renamed model doesn't compile: {:?}",
                open.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
            )));
            continue;
        }
        let after = bridge.program(open.workspace);
        let _ = bridge.dispose(open.workspace);
        match after {
            Ok(after) => {
                let expected = renamed(&base, old, &new.to_ascii_uppercase());
                let got = sorted(&after);
                if expected != got {
                    failures.push(fail(format!(
                        "structure differs:\n      expected {expected}\n      got      {got}"
                    )));
                }
            }
            Err(e) => failures.push(fail(format!("workspace.program failed: {e}"))),
        }
    }
    Some(Outcome {
        model: original.to_owned(),
        names: keys.len(),
        failures,
        outside,
    })
}

#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn s3_rename_is_safe() {
    let home = find_netlogo_home(NETLOGO_VERSION).expect("NetLogo 7.0.4");
    let jar = std::env::var_os("LOGOS_BRIDGE_JAR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo().join("bridge/build/libs/logos-bridge-0.0.0.jar"));
    let bridge =
        Bridge::spawn(&BridgeConfig::new(home.clone(), NETLOGO_VERSION, jar)).expect("bridge");
    let languages: Mutex<HashMap<Vec<String>, Option<Arc<Language>>>> = Mutex::default();
    let language = |e: &[String]| -> Option<Arc<Language>> {
        let mut cache = languages.lock().unwrap();
        cache
            .entry(e.to_vec())
            .or_insert_with(|| {
                let names: Vec<&str> = e.iter().map(String::as_str).collect();
                let t = bridge.primitives(&names).ok()?;
                Language::from_table(&t).ok().map(Arc::new)
            })
            .clone()
    };

    let (aa4, others) = candidates(&home);
    // A spread-out sample: every k-th model, skipping those that can't be copied or don't
    // compile, until there are 50.
    let stride = (others.len() / (SAMPLE * 2)).max(1);
    let order: Vec<PathBuf> = std::iter::once(aa4.clone())
        .chain(others.iter().step_by(stride).cloned())
        .chain(others.iter().cloned())
        .collect();
    let workers = std::env::var("LOGOS_WORKERS")
        .ok()
        .and_then(|w| w.parse().ok())
        .unwrap_or(4usize);
    let queue = Mutex::new(order.into_iter().enumerate());
    let outcomes: Mutex<Vec<Outcome>> = Mutex::default();
    let seen: Mutex<std::collections::HashSet<PathBuf>> = Mutex::default();
    let skipped = Mutex::new(0usize);
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    if outcomes.lock().unwrap().len() > SAMPLE {
                        break;
                    }
                    let Some((n, path)) = queue.lock().unwrap().next() else {
                        break;
                    };
                    if !seen.lock().unwrap().insert(path.clone()) {
                        continue;
                    }
                    let outcome = Copy::new(&path, n)
                        .and_then(|c| check_model(&bridge, &language, &c, &path));
                    match outcome {
                        None => *skipped.lock().unwrap() += 1,
                        Some(o) => {
                            let mut all = outcomes.lock().unwrap();
                            if all.len() <= SAMPLE || o.model == aa4 {
                                eprintln!(
                                    "{} {} ({} names, {} edits outside the code)",
                                    if o.failures.is_empty() {
                                        "ok    "
                                    } else {
                                        "FAILED"
                                    },
                                    o.model.display(),
                                    o.names,
                                    o.outside
                                );
                                for f in &o.failures {
                                    eprintln!("    {f}");
                                }
                                all.push(o);
                            }
                        }
                    }
                }
            });
        }
    });
    bridge.shutdown().unwrap();
    let outcomes = outcomes.into_inner().unwrap();
    let names: usize = outcomes.iter().map(|o| o.names).sum();
    let failed: usize = outcomes.iter().map(|o| o.failures.len()).sum();
    let has_aa4 = outcomes.iter().any(|o| o.model == aa4);
    eprintln!(
        "S3: {} models (AA4 {}), {} names renamed, {} failed; {} models skipped (no copy, \
         system dynamics, or don't compile)",
        outcomes.len(),
        if has_aa4 { "included" } else { "MISSING" },
        names,
        failed,
        skipped.into_inner().unwrap()
    );
    assert!(has_aa4);
    assert!(outcomes.len() > SAMPLE);
    assert_eq!(failed, 0);
}
