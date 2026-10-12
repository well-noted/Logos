//! The language server, driven the way an editor drives it. Needs NetLogo 7.0.4 and the bridge
//! jar:
//!
//! ```text
//! cargo test -p logos-lsp --release --test server -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `s5_responsiveness_on_aa4` measures S5: diagnostics after an edit to AA4 (target 500 ms),
//! and the outline and go to definition (target 50 ms each).

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{Duration, Instant};

use logos_bridge_client::{BridgeConfig, find_netlogo_home};
use lsp_server::{Connection, Message, Notification, Request, RequestId};
use lsp_types as lsp;
use lsp_types::notification::Notification as _;
use serde_json::{Value, json};

const NETLOGO_VERSION: &str = "7.0.4";

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn config() -> BridgeConfig {
    let home = find_netlogo_home(NETLOGO_VERSION).expect("NetLogo 7.0.4");
    let jar = std::env::var_os("LOGOS_BRIDGE_JAR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo().join("bridge/build/libs/logos-bridge-0.0.0.jar"));
    BridgeConfig::new(home, NETLOGO_VERSION, jar)
}

/// An editor's side of the connection.
struct Editor {
    conn: Connection,
    server: Option<std::thread::JoinHandle<Result<(), String>>>,
    next: i32,
    /// Notifications received while waiting for something else.
    notes: Vec<Notification>,
}

impl Editor {
    fn start(bridge: Option<BridgeConfig>) -> Editor {
        let (server_side, client_side) = Connection::memory();
        let server = std::thread::spawn(move || logos_lsp::server::serve(server_side, bridge));
        let mut e = Editor {
            conn: client_side,
            server: Some(server),
            next: 1,
            notes: Vec::new(),
        };
        let init: lsp::InitializeResult =
            serde_json::from_value(e.request_raw("initialize", json!({ "capabilities": {} })).0)
                .unwrap();
        assert!(init.capabilities.rename_provider.is_some());
        e.notify("initialized", json!({}));
        e
    }

    fn notify(&self, method: &str, params: Value) {
        self.conn
            .sender
            .send(Message::Notification(Notification::new(
                method.into(),
                params,
            )))
            .unwrap();
    }

    /// Sends a request and waits for its response; returns the result (or the error message
    /// as a string) and how long it took.
    fn request_raw(&mut self, method: &str, params: Value) -> (Value, Duration) {
        let id = RequestId::from(self.next);
        self.next += 1;
        let start = Instant::now();
        self.conn
            .sender
            .send(Message::Request(Request::new(
                id.clone(),
                method.into(),
                params,
            )))
            .unwrap();
        loop {
            match self
                .conn
                .receiver
                .recv_timeout(Duration::from_secs(120))
                .unwrap()
            {
                Message::Response(r) if r.id == id => {
                    let took = start.elapsed();
                    return match (r.result, r.error) {
                        (_, Some(e)) => (Value::String(format!("error: {}", e.message)), took),
                        (Some(v), None) => (v, took),
                        (None, None) => (Value::Null, took),
                    };
                }
                Message::Notification(n) => self.notes.push(n),
                _ => {}
            }
        }
    }

    fn request<R: lsp::request::Request>(&mut self, params: R::Params) -> (Value, Duration) {
        self.request_raw(R::METHOD, serde_json::to_value(params).unwrap())
    }

    /// Waits for the `count`th `publishDiagnostics` for version `version` of `uri`, dropping
    /// those for other versions. The server publishes each version once with the parser's
    /// errors and once more when NetLogo's check of it returns.
    fn diagnostics_for(
        &mut self,
        uri: &lsp::Uri,
        version: i32,
        count: usize,
        timeout: Duration,
    ) -> Option<Vec<lsp::Diagnostic>> {
        let mut seen = 0;
        loop {
            let d = self.next_published(uri, timeout)?;
            if d.version == Some(version) {
                seen += 1;
                if seen == count {
                    return Some(d.diagnostics);
                }
            }
        }
    }

    /// Waits for the next `publishDiagnostics` for `uri`.
    fn next_diagnostics(
        &mut self,
        uri: &lsp::Uri,
        timeout: Duration,
    ) -> Option<Vec<lsp::Diagnostic>> {
        self.next_published(uri, timeout).map(|p| p.diagnostics)
    }

    fn next_published(
        &mut self,
        uri: &lsp::Uri,
        timeout: Duration,
    ) -> Option<lsp::PublishDiagnosticsParams> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(i) = self.notes.iter().position(|n| {
                n.method == lsp::notification::PublishDiagnostics::METHOD
                    && n.params["uri"].as_str() == Some(uri.as_str())
            }) {
                let n = self.notes.remove(i);
                return Some(serde_json::from_value(n.params).unwrap());
            }
            let left = deadline.checked_duration_since(Instant::now())?;
            match self.conn.receiver.recv_timeout(left) {
                Ok(Message::Notification(n)) => self.notes.push(n),
                Ok(_) => {}
                Err(_) => return None,
            }
        }
    }

    /// Reads messages until none arrives for `quiet`.
    fn settle(&mut self, quiet: Duration) {
        while let Ok(m) = self.conn.receiver.recv_timeout(quiet) {
            if let Message::Notification(n) = m {
                self.notes.push(n);
            }
        }
    }

    fn open(&self, uri: &lsp::Uri, text: &str) {
        self.notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": uri.as_str(), "languageId": "netlogo", "version": 1, "text": text } }),
        );
    }

    fn change(&self, uri: &lsp::Uri, version: i32, text: &str) {
        self.notify(
            "textDocument/didChange",
            json!({ "textDocument": { "uri": uri.as_str(), "version": version }, "contentChanges": [ { "text": text } ] }),
        );
    }

    fn stop(mut self) {
        self.request_raw("shutdown", Value::Null);
        self.notify("exit", Value::Null);
        let result = self.server.take().unwrap().join().unwrap();
        assert_eq!(result, Ok(()));
    }
}

fn uri(p: &Path) -> lsp::Uri {
    logos_lsp::server::uri_of_path(p).unwrap()
}

fn position(text: &str, offset: usize) -> lsp::Position {
    let ix = logos_lsp::text::LineIndex::new(text);
    let (l, c) = ix.position(text, offset);
    lsp::Position::new(l, c)
}

fn at(u: &lsp::Uri, text: &str, offset: usize) -> lsp::TextDocumentPositionParams {
    lsp::TextDocumentPositionParams {
        text_document: lsp::TextDocumentIdentifier { uri: u.clone() },
        position: position(text, offset),
    }
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

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("logos-lsp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Waits until the primitive table is loaded (hover on a primitive answers).
fn wait_for_table(e: &mut Editor, u: &lsp::Uri, text: &str, primitive_at: usize) {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let (v, _) = e.request::<lsp::request::HoverRequest>(lsp::HoverParams {
            text_document_position_params: at(u, text, primitive_at),
            work_done_progress_params: Default::default(),
        });
        if !v.is_null() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the primitive table never loaded"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn editor_features_across_an_include_file() {
    let dir = temp_dir("features");
    let code = "__includes [\"lib.nls\"]\nglobals [total]\nbreed [frogs frog]\nfrogs-own [age]\n\
to setup\n  clear-all\n  create-frogs 3 [ set age 0 ]\n  set total count frogs + rate\n  grow\nend\n";
    let lib = "to grow\n  ask frogs [ set age older age ]\n  set total total + 1\nend\n\
to-report older [n]\n  let next n + 1\n  report next\nend\n";
    let text = model(code);
    std::fs::write(dir.join("m.nlogox"), &text).unwrap();
    std::fs::write(dir.join("lib.nls"), lib).unwrap();
    let mu = uri(&dir.join("m.nlogox"));
    let lu = uri(&dir.join("lib.nls"));
    let mut e = Editor::start(Some(config()));
    e.open(&mu, &text);
    let off = |s: &str, needle: &str| s.find(needle).unwrap();
    wait_for_table(&mut e, &mu, &text, off(&text, "clear-all") + 1);
    let mut failures = Vec::new();
    let mut check = |name: &str, ok: bool, detail: String| {
        eprintln!(
            "{} {name}: {detail}",
            if ok { "ok     " } else { "FAILED " }
        );
        if !ok {
            failures.push(name.to_owned());
        }
    };

    // Go to definition of `grow` (in the model) lands in lib.nls.
    let (v, _) = e.request::<lsp::request::GotoDefinition>(lsp::GotoDefinitionParams {
        text_document_position_params: at(&mu, &text, off(&text, "  grow") + 3),
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    });
    let locs: Vec<lsp::Location> = serde_json::from_value(v.clone()).unwrap_or_default();
    check(
        "definition across files",
        locs.len() == 1 && locs[0].uri == lu && locs[0].range.start == position(lib, 3),
        format!("{v}"),
    );

    // References to the global `total`: declaration, two uses in the model... in both files.
    let (v, _) = e.request::<lsp::request::References>(lsp::ReferenceParams {
        text_document_position: at(&mu, &text, off(&text, "set total") + 5),
        context: lsp::ReferenceContext {
            include_declaration: true,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    });
    let locs: Vec<lsp::Location> = serde_json::from_value(v.clone()).unwrap_or_default();
    let in_lib = locs.iter().filter(|l| l.uri == lu).count();
    check(
        "references",
        locs.len() == 4 && in_lib == 2,
        format!("{} locations, {in_lib} in lib.nls", locs.len()),
    );

    // Hover on a primitive gives its syntax; on a procedure, its header.
    let hover = |e: &mut Editor, u: &lsp::Uri, t: &str, o: usize| -> String {
        let (v, _) = e.request::<lsp::request::HoverRequest>(lsp::HoverParams {
            text_document_position_params: at(u, t, o),
            work_done_progress_params: Default::default(),
        });
        v["contents"]["value"].as_str().unwrap_or("").to_owned()
    };
    let h = hover(&mut e, &mu, &text, off(&text, "count") + 1);
    check("hover primitive", h.contains("count agentset"), h);
    let h = hover(&mut e, &lu, lib, off(lib, "older age") + 1);
    check("hover procedure", h.contains("to-report older [n]"), h);

    // Outline of the include file.
    let (v, took) = e.request::<lsp::request::DocumentSymbolRequest>(lsp::DocumentSymbolParams {
        text_document: lsp::TextDocumentIdentifier { uri: lu.clone() },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    });
    let names: Vec<String> = v
        .as_array()
        .map(|a| {
            a.iter()
                .map(|s| s["name"].as_str().unwrap_or("").to_owned())
                .collect()
        })
        .unwrap_or_default();
    check(
        "outline",
        names == ["grow", "older"],
        format!("{names:?} in {took:?}"),
    );

    // Completion offers model names and breed primitives.
    let probe = text.replacen("  grow\n", "  grow\n  cre\n", 1);
    e.change(&mu, 2, &probe);
    let (v, _) = e.request::<lsp::request::Completion>(lsp::CompletionParams {
        text_document_position: at(&mu, &probe, off(&probe, "  cre\n") + 5),
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    });
    let labels: Vec<String> = v
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| c["label"].as_str().unwrap_or("").to_owned())
                .collect()
        })
        .unwrap_or_default();
    check(
        "completion",
        labels.contains(&"create-frogs".to_owned())
            && labels.contains(&"create-turtles".to_owned()),
        format!("{labels:?}"),
    );
    e.change(&mu, 3, &text);

    // Rename the reporter from its use in the model's include file: both definition and uses.
    let (v, _) =
        e.request::<lsp::request::PrepareRenameRequest>(at(&lu, lib, off(lib, "older age") + 2));
    check("prepare rename", v.get("start").is_some(), format!("{v}"));
    let (v, _) = e.request::<lsp::request::Rename>(lsp::RenameParams {
        text_document_position: at(&lu, lib, off(lib, "older age") + 2),
        new_name: "aged".into(),
        work_done_progress_params: Default::default(),
    });
    let edits: lsp::WorkspaceEdit = serde_json::from_value(v.clone()).unwrap_or_default();
    let n = edits
        .changes
        .as_ref()
        .map_or(0, |c| c.values().map(Vec::len).sum());
    check("rename", n == 2, format!("{v}"));
    // Renaming to a primitive's name is refused.
    let (v, _) = e.request::<lsp::request::Rename>(lsp::RenameParams {
        text_document_position: at(&mu, &text, off(&text, "set total") + 5),
        new_name: "count".into(),
        work_done_progress_params: Default::default(),
    });
    check(
        "rename refused",
        v.as_str().is_some_and(|s| s.starts_with("error")),
        format!("{v}"),
    );
    // An interface global can't be renamed in code.
    let (v, _) =
        e.request::<lsp::request::PrepareRenameRequest>(at(&mu, &text, off(&text, "rate") + 1));
    check(
        "interface global",
        v.as_str().is_some_and(|s| s.contains("interface")),
        format!("{v}"),
    );

    // An error in the include file, reported by NetLogo there once the file is saved.
    let bad = lib.replace("set total total + 1", "set total total + \"x\"");
    std::fs::write(dir.join("lib.nls"), &bad).unwrap();
    e.settle(Duration::from_millis(500));
    e.notes.clear();
    e.open(&lu, &bad);
    let mut found = None;
    while let Some(d) = e.next_diagnostics(&lu, Duration::from_secs(30)) {
        if !d.is_empty() {
            found = Some(d);
            break;
        }
    }
    let d = found.unwrap_or_default();
    check(
        "include diagnostics",
        d.len() == 1 && d[0].source.as_deref() == Some("netlogo"),
        format!(
            "{:?}",
            d.iter().map(|d| (&d.message, d.range)).collect::<Vec<_>>()
        ),
    );

    e.stop();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(failures.is_empty(), "failed: {failures:?}");
}

#[test]
#[ignore = "needs NetLogo 7.0.4 and the bridge jar"]
fn s5_responsiveness_on_aa4() {
    let path = repo().join("corpus/own/aa4-combined/AA4-Combined.nlogox");
    let text = std::fs::read_to_string(&path).unwrap();
    let u = uri(&path);
    let started = Instant::now();
    let mut e = Editor::start(Some(config()));
    e.open(&u, &text);
    let first = text.find("ask ").expect("an ask in AA4");
    wait_for_table(&mut e, &u, &text, first + 1);
    e.settle(Duration::from_secs(3));
    e.notes.clear();
    eprintln!(
        "S5: server ready with the primitive table after {:?}",
        started.elapsed()
    );

    // Type a new procedure with a NetLogo compile error at the end of the code, one keystroke
    // at a time, 60 ms apart, then measure from the last keystroke to NetLogo's diagnostics.
    let end = text.find("]]></code>").expect("code in CDATA");
    let typed = "\nto logos-s5-probe\n  fd \"x\"\nend\n";
    let mut version = 1;
    let mut last = Instant::now();
    for i in (typed.len() - 8)..=typed.len() {
        let edited = format!("{}{}{}", &text[..end], &typed[..i], &text[end..]);
        if version > 1 {
            std::thread::sleep(Duration::from_millis(60));
        }
        version += 1;
        e.change(&u, version, &edited);
        last = Instant::now();
    }
    // The last version is published twice: the parser's errors at once, then NetLogo's.
    let deadline = Duration::from_secs(30);
    let parser = e
        .diagnostics_for(&u, version, 1, deadline)
        .expect("the parser's diagnostics");
    let parser_time = last.elapsed();
    let d = e
        .diagnostics_for(&u, version, 1, deadline)
        .expect("NetLogo's diagnostics");
    let diag_time = last.elapsed();
    let found = d
        .iter()
        .any(|d| d.source.as_deref() == Some("netlogo") && d.message.contains("FD"));
    eprintln!(
        "S5: after the last keystroke: the parser's diagnostics in {parser_time:?} ({}), \
         NetLogo's in {diag_time:?} (target 500 ms): {:?}",
        parser.len(),
        d.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    // And when the error is removed again.
    version += 1;
    e.change(&u, version, &text);
    let removed = Instant::now();
    let d = e
        .diagnostics_for(&u, version, 2, deadline)
        .expect("diagnostics after removing it");
    let clear_time = removed.elapsed();
    eprintln!(
        "S5: diagnostics after removing it: {clear_time:?}, {} left",
        d.len()
    );

    // Outline and go to definition, 20 times each.
    let mut outline = Vec::new();
    let mut definition = Vec::new();
    let call = procedure_call(&text).expect("a call of one of AA4's procedures");
    for _ in 0..20 {
        let (_, t) = e.request::<lsp::request::DocumentSymbolRequest>(lsp::DocumentSymbolParams {
            text_document: lsp::TextDocumentIdentifier { uri: u.clone() },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        });
        outline.push(t);
        let (v, t) = e.request::<lsp::request::GotoDefinition>(lsp::GotoDefinitionParams {
            text_document_position_params: at(&u, &text, call + 1),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        });
        assert!(
            v.as_array().is_some_and(|a| !a.is_empty()),
            "no definition for {:?}: {v}",
            &text[call..call + 30]
        );
        definition.push(t);
    }
    outline.sort();
    definition.sort();
    eprintln!(
        "S5: outline median {:?}, max {:?}; definition median {:?}, max {:?} (target 50 ms)",
        outline[10], outline[19], definition[10], definition[19]
    );
    e.stop();
    let ok = found
        && diag_time <= Duration::from_millis(500)
        && clear_time <= Duration::from_millis(500)
        && d.is_empty()
        && outline[19] <= Duration::from_millis(50)
        && definition[19] <= Duration::from_millis(50);
    eprintln!("S5: {}", if ok { "met" } else { "NOT MET" });
    assert!(ok);
}

/// The offset in the model file of a call of one of the procedures defined in its code.
fn procedure_call(file: &str) -> Option<usize> {
    let base = file.find("<code><![CDATA[")? + "<code><![CDATA[".len();
    let text = &file[base..file.find("]]></code>")?];
    let names: Vec<&str> = text
        .lines()
        .filter_map(|l| {
            l.strip_prefix("to-report ")
                .or_else(|| l.strip_prefix("to "))
        })
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    let delimiter = |c: Option<char>| c.is_none_or(|c| c.is_whitespace() || "[]()".contains(c));
    for name in names {
        for (i, _) in text.match_indices(name) {
            let before = text[..i].chars().next_back();
            let after = text[i + name.len()..].chars().next();
            let header = text[..i].ends_with("to ") || text[..i].ends_with("to-report ");
            let line_start = text[..i].rfind('\n').map_or(0, |n| n + 1);
            let comment = text[line_start..i].contains(';');
            if delimiter(before) && delimiter(after) && !header && !comment {
                return Some(base + i);
            }
        }
    }
    None
}

#[test]
fn starts_and_stops_without_netlogo() {
    let mut e = Editor::start(None);
    let code = "to setup\n  go2\nend\nto go2\nend\n";
    let u = lsp::Uri::from_str("untitled:Untitled-1.nls").unwrap();
    e.open(&u, code);
    let (v, _) = e.request::<lsp::request::DocumentSymbolRequest>(lsp::DocumentSymbolParams {
        text_document: lsp::TextDocumentIdentifier { uri: u.clone() },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    });
    assert_eq!(v.as_array().map(Vec::len), Some(2), "{v}");
    let broken = "to setup\n  ask turtles [ fd 1\nend\n";
    e.change(&u, 2, broken);
    let mut last = Vec::new();
    while let Some(d) = e.next_diagnostics(&u, Duration::from_secs(2)) {
        last = d;
    }
    assert!(!last.is_empty());
    assert_eq!(last[0].source.as_deref(), Some("logos"));
    e.stop();
}
