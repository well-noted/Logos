//! Spike B round-trip tests over the corpus (docs/phase-0-spec.md, B1-B3).
//!
//! The corpus here is AA4-Combined from `corpus/own`, every `.nlogox` in the NetLogo 7.0.4
//! Models Library when NetLogo is installed (`LOGOS_NETLOGO_HOME` or the default location),
//! and every `.nlogox` under the folders in `LOGOS_CORPUS_DIRS` (separated like `PATH`).
//!
//! ```text
//! cargo test -p logos-nlogox --test corpus -- --nocapture
//! cargo test -p logos-nlogox --test corpus -- --ignored --nocapture   # B3, needs the bridge
//! ```

use std::ops::Range;
use std::path::{Path, PathBuf};

use logos_nlogox::{Element, Nlogox, Node};

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

/// Every corpus file available on this machine, and a description of where they came from.
fn corpus() -> (Vec<PathBuf>, String) {
    let mut files = vec![repo().join("corpus/own/aa4-combined/AA4-Combined.nlogox")];
    let mut sources = vec!["AA4-Combined".to_owned()];
    if let Some(home) = logos_bridge_client::find_netlogo_home(NETLOGO_VERSION) {
        let before = files.len();
        walk(&home.join("models"), &mut files);
        sources.push(format!("Models Library {}", files.len() - before));
    }
    if let Some(dirs) = std::env::var_os("LOGOS_CORPUS_DIRS") {
        for d in std::env::split_paths(&dirs) {
            let before = files.len();
            walk(&d, &mut files);
            sources.push(format!("{} {}", d.display(), files.len() - before));
        }
    }
    files.sort();
    files.dedup();
    (files, sources.join(", "))
}

/// Checks that an element's children cover its content with no gaps or overlaps, recursively.
fn check_tiling(f: &Nlogox, e: &Element, path: &Path) {
    let Some(content) = &e.content else {
        assert!(e.children.is_empty());
        return;
    };
    let mut at = content.start;
    for n in &e.children {
        let span: &Range<usize> = match n {
            Node::Element(c) => {
                assert_eq!(c.start_tag.start, c.span.start);
                for a in &c.attributes {
                    assert!(a.value_span.end <= c.start_tag.end);
                }
                check_tiling(f, c, path);
                &c.span
            }
            Node::Text(s) | Node::CData(s) | Node::Comment(s) | Node::Other(s) => s,
        };
        assert_eq!(
            span.start,
            at,
            "{}: gap in <{}> at byte {at}",
            path.display(),
            e.name
        );
        at = span.end;
    }
    assert_eq!(
        at,
        content.end,
        "{}: <{}> content not covered",
        path.display(),
        e.name
    );
    let _ = f;
}

/// B1: reading and writing an unmodified file gives the same bytes, and the parse accounts for
/// every byte of every element.
#[test]
fn b1_unmodified_round_trip() {
    let (files, sources) = corpus();
    let mut failures = Vec::new();
    for path in &files {
        let bytes = std::fs::read(path).unwrap();
        let text = match String::from_utf8(bytes.clone()) {
            Ok(t) => t,
            Err(e) => {
                failures.push(format!("{}: not UTF-8: {e}", path.display()));
                continue;
            }
        };
        let f = match Nlogox::parse(text) {
            Ok(f) => f,
            Err(e) => {
                failures.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        assert_eq!(f.text().as_bytes(), bytes, "{}", path.display());
        assert_eq!(f.root().name, "model", "{}", path.display());
        check_tiling(&f, f.root(), path);
        if let Err(e) = f.code() {
            failures.push(format!("{}: code: {e}", path.display()));
        }
        if let Err(e) = f.info() {
            failures.push(format!("{}: info: {e}", path.display()));
        }
    }
    eprintln!(
        "B1: {} files ({sources}), {} byte-identical, {} failures",
        files.len(),
        files.len() - failures.len(),
        failures.len()
    );
    for m in failures.iter().take(20) {
        eprintln!("  {m}");
    }
    assert!(failures.is_empty());
}

/// The range of `before` that differs from `after`, assuming one contiguous change.
fn changed_range(before: &str, after: &str) -> Range<usize> {
    let p = before
        .bytes()
        .zip(after.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let s = before[p..]
        .bytes()
        .rev()
        .zip(after[p..].bytes().rev())
        .take_while(|(a, b)| a == b)
        .count();
    p..before.len() - s
}

fn within(r: &Range<usize>, outer: &Range<usize>) -> bool {
    r.start >= outer.start && r.end <= outer.end
}

const ADDED_LINE: &str = "; logos round-trip test";

/// A new slider maximum: one more than a number, otherwise the expression plus one.
fn bumped(max: &str) -> String {
    match max.trim().parse::<f64>() {
        Ok(n) => format!("{}", n + 1.0),
        Err(_) => format!("({max}) + 1"),
    }
}

/// Applies the B2 edits to `text`, checking that each changes only its own bytes. Returns the
/// edited file and whether a slider was changed.
fn b2_edit(text: &str, path: &Path) -> Result<(String, bool), String> {
    let mut f = Nlogox::parse(text).map_err(|e| e.to_string())?;
    let code = f.code().map_err(|e| e.to_string())?;
    let code_el = f.section("code").ok_or("no <code>")?;
    let code_region = code_el.content.clone().unwrap_or(code_el.span.clone());
    let new_code = format!("{code}\n{ADDED_LINE}");
    let before = f.text().to_owned();
    f.set_code(&new_code).map_err(|e| e.to_string())?;
    let r = changed_range(&before, f.text());
    if !within(&r, &code_region) {
        return Err(format!(
            "code edit changed bytes {r:?} outside {code_region:?}"
        ));
    }
    if f.code().map_err(|e| e.to_string())? != new_code {
        return Err("code does not read back".into());
    }

    let slider = f.widgets().iter().position(|w| w.name == "slider");
    if let Some(i) = slider {
        let w = f.widgets()[i];
        let Some(max) = w.attribute("max") else {
            return Err(format!("slider {i} has no max"));
        };
        let value_span = max.value_span.clone();
        let new_max = bumped(&f.attribute_value(w, "max").unwrap());
        let before = f.text().to_owned();
        let wp = f.widget_path(i).unwrap();
        f.set_attribute(&wp, "max", &new_max)
            .map_err(|e| e.to_string())?;
        let r = changed_range(&before, f.text());
        if !within(&r, &value_span) {
            return Err(format!(
                "slider edit changed bytes {r:?} outside {value_span:?}"
            ));
        }
        if f.attribute_value(f.widgets()[i], "max").as_deref() != Some(new_max.as_str()) {
            return Err("slider max does not read back".into());
        }
    }
    let _ = path;
    Ok((f.into_text(), slider.is_some()))
}

/// B2: edit round-trip. Replacing the code with itself plus a comment line, and changing one
/// slider's `max`, changes only the bytes of those two parts.
#[test]
fn b2_edit_round_trip() {
    let (files, sources) = corpus();
    let mut failures = Vec::new();
    let mut sliders = 0;
    for path in &files {
        let text = std::fs::read_to_string(path).unwrap();
        match b2_edit(&text, path) {
            Ok((_, slider)) => sliders += usize::from(slider),
            Err(e) => failures.push(format!("{}: {e}", path.display())),
        }
    }
    eprintln!(
        "B2: {} files ({sources}), {} with a slider; edits confined to their regions in {}, \
         {} failures",
        files.len(),
        sliders,
        files.len() - failures.len(),
        failures.len()
    );
    for m in failures.iter().take(20) {
        eprintln!("  {m}");
    }
    assert!(failures.is_empty());
}

/// B3: NetLogo accepts edited files. Each B2 output is written next to a copy of the model's
/// `.nls` include files and opened in NetLogo 7.0.4 through the bridge; it must compile exactly
/// as the original does (models that need an extension NetLogo can't find fail the same way
/// both times).
#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn b3_netlogo_accepts_edited_files() {
    use logos_bridge_client::{Bridge, BridgeConfig, find_netlogo_home};
    use std::sync::Mutex;

    let (files, sources) = corpus();
    let home = find_netlogo_home(NETLOGO_VERSION).expect("NetLogo 7.0.4");
    let jar = repo().join("bridge/build/libs/logos-bridge-0.0.0.jar");
    let bridge = Bridge::spawn(&BridgeConfig::new(home, NETLOGO_VERSION, jar)).expect("bridge");
    let out = std::env::temp_dir().join(format!("logos b3 {}", std::process::id()));
    std::fs::create_dir_all(&out).unwrap();
    let workers = std::env::var("LOGOS_WORKERS")
        .ok()
        .and_then(|w| w.parse().ok())
        .unwrap_or(4usize);

    let queue = Mutex::new(files.iter().enumerate());
    let results: Mutex<Vec<(PathBuf, Result<bool, String>)>> = Mutex::new(Vec::new());
    // (compiled, errors). A file NetLogo refuses to open (for example NetLogo's own test
    // fixtures without a view) counts as not compiling, with the open error as its error.
    let compile = |path: &Path| -> Result<(bool, Vec<String>), String> {
        match bridge.open(path) {
            Ok(r) => {
                let errors = r.errors.iter().map(|e| format!("{e:?}")).collect();
                let _ = bridge.dispose(r.workspace);
                Ok((r.compiled, errors))
            }
            Err(logos_bridge_client::ClientError::Rpc(e)) => {
                Ok((false, vec![format!("open failed: {}", e.message)]))
            }
            Err(e) => Err(e.to_string()),
        }
    };
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let Some((i, path)) = queue.lock().unwrap().next() else {
                        break;
                    };
                    let text = std::fs::read_to_string(path).unwrap();
                    let result = (|| {
                        let (edited, _) = b2_edit(&text, path)?;
                        let dir = out.join(i.to_string());
                        std::fs::create_dir_all(&dir).unwrap();
                        // __includes paths are relative to the model's folder.
                        let src_dir = path.parent().unwrap();
                        for e in std::fs::read_dir(src_dir).unwrap().flatten() {
                            if e.path().extension().and_then(|x| x.to_str()) == Some("nls") {
                                std::fs::copy(e.path(), dir.join(e.file_name())).unwrap();
                            }
                        }
                        let copy = dir.join(path.file_name().unwrap());
                        std::fs::write(&copy, edited).unwrap();
                        let original = compile(path)?;
                        let edited = compile(&copy)?;
                        if original != edited {
                            return Err(format!(
                                "original compiled={} {:?}, edited compiled={} {:?}",
                                original.0, original.1, edited.0, edited.1
                            ));
                        }
                        Ok(original.0)
                    })();
                    results.lock().unwrap().push((path.clone(), result));
                }
            });
        }
    });
    bridge.shutdown().unwrap();
    let results = results.into_inner().unwrap();
    let compiled = results.iter().filter(|r| matches!(r.1, Ok(true))).count();
    let failures: Vec<_> = results
        .iter()
        .filter_map(|(p, r)| r.as_ref().err().map(|e| format!("{}: {e}", p.display())))
        .collect();
    let not_compiling: Vec<_> = results
        .iter()
        .filter(|r| matches!(r.1, Ok(false)))
        .map(|r| r.0.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    eprintln!(
        "B3: {} files ({sources}): {compiled} compile before and after editing; {} don't \
         compile either way {:?}; {} differ",
        results.len(),
        not_compiling.len(),
        not_compiling.iter().take(10).collect::<Vec<_>>(),
        failures.len()
    );
    for f in failures.iter().take(20) {
        eprintln!("  {f}");
    }
    assert!(failures.is_empty());
}
