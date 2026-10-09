//! Phase 0 A8 (`engine.primitives`) and A9 (`model.convertLegacy`) against the installed
//! NetLogo 7.0.4. Ignored by default:
//!
//! ```text
//! cargo test -p logos-bridge-client --test model_tools -- --ignored --test-threads=1 --nocapture
//! ```
//!
//! A8 loads `nw` and `table` unless `LOGOS_A8_EXTENSIONS` lists others (comma-separated).
//! A9 converts each `.nlogo` in `corpus/legacy` (or `LOGOS_LEGACY_DIR`) that has a GUI
//! conversion next to it, named `<stem>-gui-7.0.4.nlogox`, and compares the two after sorting
//! each element's attributes.

use std::path::{Path, PathBuf};

use logos_bridge_client::protocol::PrimitiveKind;
use logos_bridge_client::{Bridge, BridgeConfig, find_netlogo_home};
use quick_xml::events::Event;

const NETLOGO_VERSION: &str = "7.0.4";

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn spawn() -> Bridge {
    let home = find_netlogo_home(NETLOGO_VERSION).expect("NetLogo 7.0.4; set LOGOS_NETLOGO_HOME");
    let jar = repo().join("bridge/build/libs/logos-bridge-0.0.0.jar");
    Bridge::spawn(&BridgeConfig::new(home, NETLOGO_VERSION, jar)).expect("bridge")
}

#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn a8_primitive_table_with_extensions() {
    let names = std::env::var("LOGOS_A8_EXTENSIONS").unwrap_or_else(|_| "nw,table".into());
    let extensions: Vec<&str> = names.split(',').map(str::trim).collect();
    let bridge = spawn();
    let table = bridge.primitives(&extensions).unwrap();
    assert_eq!(table.netlogo_version, "NetLogo 7.0.4");
    let core = table
        .primitives
        .iter()
        .filter(|p| p.extension.is_none())
        .count();
    assert_eq!(core, 459, "core primitives in NetLogo 7.0.4");
    let fd = table
        .primitives
        .iter()
        .find(|p| p.name == "fd")
        .expect("fd");
    assert_eq!(fd.kind, PrimitiveKind::Command);
    assert_eq!(fd.agent_class, "-T--");
    let mut counts = Vec::new();
    for ext in &extensions {
        let n = table
            .primitives
            .iter()
            .filter(|p| p.extension.as_deref() == Some(*ext))
            .count();
        assert!(n > 0, "no primitives for extension {ext}");
        assert!(
            table
                .primitives
                .iter()
                .any(|p| p.name.starts_with(&format!("{ext}:"))),
            "{ext}: names carry the prefix"
        );
        counts.push(format!("{ext} {n}"));
    }
    eprintln!(
        "A8: {} primitives: core {core}, {}",
        table.primitives.len(),
        counts.join(", ")
    );
    bridge.shutdown().unwrap();
}

/// The document as a list of events with attributes sorted by name, so attribute order does
/// not count as a difference. Everything else (text, whitespace, CDATA, element order) does.
fn normalized(xml: &str) -> Vec<String> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut out = Vec::new();
    let tag = |e: &quick_xml::events::BytesStart| {
        let mut attrs: Vec<String> = e
            .attributes()
            .map(|a| {
                let a = a.expect("well-formed attribute");
                format!(
                    "{}={:?}",
                    String::from_utf8_lossy(a.key.as_ref()),
                    String::from_utf8_lossy(&a.value)
                )
            })
            .collect();
        attrs.sort();
        format!(
            "<{} {}>",
            String::from_utf8_lossy(e.name().as_ref()),
            attrs.join(" ")
        )
    };
    loop {
        match reader.read_event().expect("well-formed XML") {
            Event::Start(e) => out.push(tag(&e)),
            Event::Empty(e) => {
                out.push(tag(&e));
                out.push(format!("</{}>", String::from_utf8_lossy(e.name().as_ref())));
            }
            Event::End(e) => out.push(format!("</{}>", String::from_utf8_lossy(e.name().as_ref()))),
            Event::Text(t) => out.push(format!("text {:?}", String::from_utf8_lossy(&t))),
            Event::CData(t) => out.push(format!("cdata {:?}", String::from_utf8_lossy(&t))),
            Event::GeneralRef(r) => out.push(format!("ref {:?}", String::from_utf8_lossy(&r))),
            Event::Decl(d) => out.push(format!("decl {:?}", String::from_utf8_lossy(&d))),
            Event::Comment(c) => out.push(format!("comment {:?}", String::from_utf8_lossy(&c))),
            Event::PI(p) => out.push(format!("pi {:?}", String::from_utf8_lossy(&p))),
            Event::DocType(d) => out.push(format!("doctype {:?}", String::from_utf8_lossy(&d))),
            Event::Eof => break,
        }
    }
    out
}

/// None when equal, otherwise a description of the first difference.
fn first_difference(ours: &str, gui: &str) -> Option<String> {
    let (a, b) = (normalized(ours), normalized(gui));
    let i = a.iter().zip(&b).position(|(x, y)| x != y);
    match i {
        None if a.len() == b.len() => None,
        None => Some(format!(
            "one document ends early: {} events here, {} in the GUI's",
            a.len(),
            b.len()
        )),
        Some(i) => {
            let show = |v: &[String]| {
                v[i.saturating_sub(2)..(i + 3).min(v.len())]
                    .iter()
                    .map(|s| format!("    {}", s.chars().take(200).collect::<String>()))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            Some(format!(
                "event {i} differs\n  converted here:\n{}\n  GUI:\n{}",
                show(&a),
                show(&b)
            ))
        }
    }
}

fn legacy_dir() -> PathBuf {
    std::env::var_os("LOGOS_LEGACY_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo().join("corpus/legacy"))
}

/// Pairs of (source `.nlogo`, GUI conversion) found in `dir`.
fn cases(dir: &Path) -> Vec<(PathBuf, PathBuf)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("nlogo") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let gui = dir.join(format!("{stem}-gui-7.0.4.nlogox"));
        if gui.exists() {
            out.push((path, gui));
        }
    }
    out.sort();
    out
}

#[test]
#[ignore = "needs NetLogo 7.0.4, the bridge jar and GUI conversions in corpus/legacy"]
fn a9_conversion_matches_the_netlogo_gui() {
    let dir = legacy_dir();
    let cases = cases(&dir);
    assert!(
        !cases.is_empty(),
        "no <name>.nlogo with <name>-gui-7.0.4.nlogox in {}",
        dir.display()
    );
    let out_dir = std::env::temp_dir().join(format!("logos a9 {}", std::process::id()));
    std::fs::create_dir_all(&out_dir).unwrap();
    let bridge = spawn();
    let mut failures = Vec::new();
    for (source, gui) in &cases {
        let before = std::fs::read(source).unwrap();
        let output = out_dir.join(
            gui.file_name()
                .unwrap()
                .to_string_lossy()
                .replace("-gui-", "-logos-"),
        );
        let _ = std::fs::remove_file(&output);
        let r = bridge.convert_legacy(source, &output).unwrap();
        assert_eq!(std::fs::read(source).unwrap(), before, "source unchanged");
        let ours = std::fs::read_to_string(&output).unwrap();
        let theirs = std::fs::read_to_string(gui).unwrap();
        let name = source.file_name().unwrap().to_string_lossy();
        match first_difference(&ours, &theirs) {
            None => eprintln!(
                "A9: {name} ({}): identical to the GUI's conversion after sorting attributes; \
                 warnings {:?}",
                r.source_version, r.warnings
            ),
            Some(d) => {
                eprintln!(
                    "A9: {name} ({}): differs from the GUI's conversion; {d}",
                    r.source_version
                );
                eprintln!("    ours is at {}", output.display());
                failures.push(name.into_owned());
            }
        }
    }
    bridge.shutdown().unwrap();
    assert!(failures.is_empty(), "differ from the GUI: {failures:?}");
}

#[test]
fn normalization_ignores_attribute_order_only() {
    let a = r#"<?xml version="1.0"?><m a="1" b="2"><c x="y"/>t</m>"#;
    assert_eq!(
        first_difference(
            a,
            r#"<?xml version="1.0"?><m b="2" a="1"><c x="y"></c>t</m>"#
        ),
        None
    );
    assert!(
        first_difference(a, r#"<?xml version="1.0"?><m a="1" b="3"><c x="y"/>t</m>"#).is_some()
    );
    assert!(
        first_difference(a, r#"<?xml version="1.0"?><m a="1" b="2"><c x="y"/>u</m>"#).is_some()
    );
    assert!(first_difference(a, r#"<?xml version="1.0"?><m a="1" b="2"><c x="y"/></m>"#).is_some());
}
