//! S4: compile errors from `workspace.check`, mapped into the editor's text, point at the
//! same place NetLogo reports when it opens the model with that code. Needs NetLogo 7.0.4:
//!
//! ```text
//! cargo test -p logos-lsp --test s4 -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use logos_bridge_client::{Bridge, BridgeConfig, find_netlogo_home};
use logos_lsp::diagnostics;

const NETLOGO_VERSION: &str = "7.0.4";

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn model(code: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<model version="NetLogo 7.0.4" snapToGrid="false">
  <code><![CDATA[{code}]]></code>
  <widgets>
    <view x="0" wrappingAllowedX="true" y="0" frameRate="30.0" minPycor="-5" height="110" showTickCounter="true" patchSize="10.0" fontSize="10" wrappingAllowedY="true" width="110" tickCounterLabel="ticks" maxPycor="5" updateMode="1" maxPxcor="5" minPxcor="-5"></view>
    <slider x="0" y="0" width="100" height="40" variable="rate" min="0" max="10" step="1" default="5" direction="Horizontal" display="rate"></slider>
  </widgets>
  <info></info>
  <turtleShapes></turtleShapes>
  <linkShapes></linkShapes>
</model>
"#
    )
}

#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn s4_diagnostic_positions_match_netlogo() {
    let home = find_netlogo_home(NETLOGO_VERSION).expect("NetLogo 7.0.4");
    let jar = repo().join("bridge/build/libs/logos-bridge-0.0.0.jar");
    let bridge = Bridge::spawn(&BridgeConfig::new(home, NETLOGO_VERSION, jar)).expect("bridge");
    // (code, the text the error should point at, if known)
    let cases: &[(&str, Option<&str>)] = &[
        ("to setup\n  fd foo\nend\n", Some("foo")),
        ("to setup\n  fd\nend\n", Some("fd")),
        ("to setup\n  fd \"a\"\nend\n", Some("\"a\"")),
        ("to setup\n  ask turtles [ fd 1\nend\n", None),
        ("to setup\n  go2\nend\n", Some("go2")),
        (
            "; caf\u{e9} \u{1F600}\nto setup\n  fd foo\nend\n",
            Some("foo"),
        ),
        (
            "to-report r\n  report count turtles with [ colr = red ]\nend\n",
            Some("colr"),
        ),
        ("globals [g]\nto setup\n  set g rate + \"x\"\nend\n", None),
        ("to setup\n  crt 1 [ fd 1 ] ]\nend\n", None),
    ];
    let base = bridge.open_string(&model("to setup end")).unwrap();
    assert!(base.compiled);
    let mut failed = 0;
    for (code, expected) in cases {
        let checked = bridge.check(base.workspace, code).unwrap();
        let opened = bridge.open_string(&model(code)).unwrap();
        let _ = bridge.dispose(opened.workspace);
        let ours = diagnostics(code, &checked.errors, &[]);
        let netlogo = diagnostics(code, &opened.errors, &[]);
        let text = |d: &[logos_lsp::Diagnostic]| -> Vec<String> {
            d.iter()
                .map(|d| format!("{:?} {:?}", &code[d.range.clone()], d.message))
                .collect()
        };
        let ok = !ours.is_empty()
            && ours == netlogo
            && expected.is_none_or(|e| &code[ours[0].range.clone()] == e);
        eprintln!(
            "{} {:?}\n    check: {:?}\n    open:  {:?}",
            if ok { "same   " } else { "DIFFERS" },
            code.lines().last().unwrap_or(""),
            text(&ours),
            text(&netlogo)
        );
        if !ok {
            failed += 1;
        }
    }
    bridge.shutdown().unwrap();
    assert_eq!(failed, 0);
}
