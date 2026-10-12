//! The Code tab's connection to the language server (design 4.2, Part S of Phase 1).
//!
//! `logos_lsp::server::serve` runs in this process on an in-memory connection, and starts its
//! own check bridge with NetLogo's bytecode generator off (D-25). The language server works on
//! whole `.nlogox` files; the Code tab edits only the code. This proxy sits between them: the
//! editor's document is the model's code under the URI `<model file URI>#code`, and the proxy
//! turns it into the whole file, with every position converted between the code and the file.
//!
//! Edits a rename makes outside the code (in widgets and BehaviorSpace experiments) are
//! applied to the model document here, and the front end is told to reload the widgets.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use logos_bridge_client::BridgeConfig;
use logos_lsp::server::{key_of, uri_of_path};
use logos_lsp::text::LineIndex;
use logos_nlogox::{Decoded, Nlogox};
use lsp_server::{Connection, Message};
use serde_json::{Value as Json, json};

use crate::session::Emit;

const CODE_SUFFIX: &str = "#code";

/// One model as the language server sees it.
struct ModelDoc {
    path: PathBuf,
    /// The URI the server knows the file by.
    file_uri: String,
    /// The whole file, with the editor's code in it.
    file_text: String,
    file_index: LineIndex,
    code: String,
    code_index: LineIndex,
    decoded: Decoded,
    /// Byte range of the code section's content in `file_text`.
    content: std::ops::Range<usize>,
}

impl ModelDoc {
    fn build(path: &Path, file_uri: String, base: &str, code: &str) -> Option<ModelDoc> {
        let mut n = Nlogox::parse(base.to_string()).ok()?;
        n.set_code(code).ok()?;
        let file_text = n.text().to_string();
        let content = n.section("code")?.content.clone()?;
        let decoded = n.code_mapped()?.ok()?;
        Some(ModelDoc {
            path: path.to_path_buf(),
            file_uri,
            file_index: LineIndex::new(&file_text),
            code: decoded.text.clone(),
            code_index: LineIndex::new(&decoded.text),
            decoded,
            content,
            file_text,
        })
    }

    fn to_file(&self, line: u32, col: u32, end: bool) -> (u32, u32) {
        let off = self.code_index.offset(&self.code, line, col);
        let f = if end {
            self.decoded.to_file_end(off)
        } else {
            self.decoded.to_file(off)
        };
        self.file_index.position(&self.file_text, f)
    }

    fn to_code(&self, line: u32, col: u32) -> Option<(u32, u32)> {
        let f = self.file_index.offset(&self.file_text, line, col);
        if f < self.content.start || f > self.content.end {
            return None;
        }
        let off = self.decoded.from_file(f);
        Some(self.code_index.position(&self.code, off))
    }

    /// A file range as a code range, or None when it is outside the code.
    fn range_to_code(&self, r: &Json) -> Option<Json> {
        let (sl, sc) = pos(r.get("start")?)?;
        let (el, ec) = pos(r.get("end")?)?;
        let (sl, sc) = self.to_code(sl, sc)?;
        let (el, ec) = self.to_code(el, ec)?;
        Some(
            json!({ "start": { "line": sl, "character": sc }, "end": { "line": el, "character": ec } }),
        )
    }

    /// A file range as a byte range in the file.
    fn range_bytes(&self, r: &Json) -> Option<std::ops::Range<usize>> {
        let (sl, sc) = pos(r.get("start")?)?;
        let (el, ec) = pos(r.get("end")?)?;
        Some(
            self.file_index.offset(&self.file_text, sl, sc)
                ..self.file_index.offset(&self.file_text, el, ec),
        )
    }
}

fn pos(p: &Json) -> Option<(u32, u32)> {
    Some((
        p.get("line")?.as_u64()? as u32,
        p.get("character")?.as_u64()? as u32,
    ))
}

fn is_range(v: &Json) -> bool {
    v.get("start").and_then(|s| s.get("line")).is_some()
        && v.get("end").and_then(|s| s.get("line")).is_some()
}

/// Edits a rename made outside the code, for the host to apply: (model path, file byte range,
/// new text), against the file text the server saw.
pub type OutsideEdits = Vec<(PathBuf, std::ops::Range<usize>, String)>;

pub struct LspProxy {
    to_server: crossbeam_channel::Sender<Message>,
    docs: Mutex<HashMap<String, ModelDoc>>,
    /// Methods of requests in flight from the editor, by id.
    pending: Mutex<HashMap<String, String>>,
    /// Applies rename edits outside the code to the model document; returns whether it did.
    apply_outside: Arc<dyn Fn(OutsideEdits, String) -> bool + Send + Sync>,
}

impl LspProxy {
    /// Starts the language server in this process. `bridge` is how its check bridge starts.
    pub fn start(
        bridge: Option<BridgeConfig>,
        emit: Emit,
        apply_outside: Arc<dyn Fn(OutsideEdits, String) -> bool + Send + Sync>,
    ) -> Arc<LspProxy> {
        let (server, client) = Connection::memory();
        std::thread::Builder::new()
            .name("logos-lsp".into())
            .spawn(move || {
                if let Err(e) = logos_lsp::server::serve(server, bridge) {
                    eprintln!("language server stopped: {e}");
                }
            })
            .expect("start the language server");
        let proxy = Arc::new(LspProxy {
            to_server: client.sender.clone(),
            docs: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            apply_outside,
        });
        let p = proxy.clone();
        std::thread::Builder::new()
            .name("logos-lsp-proxy".into())
            .spawn(move || {
                for msg in client.receiver {
                    if let Some(out) = p.server_message(msg) {
                        emit("lsp", Json::String(out));
                    }
                }
            })
            .expect("start the language server proxy");
        proxy
    }

    /// The editor's URI for a model's code.
    pub fn code_uri(path: &Path) -> Option<String> {
        Some(format!("{}{CODE_SUFFIX}", uri_of_path(path)?.as_str()))
    }

    /// A message from the editor. `model_text` gives the current file text of a model by path
    /// (with code, widgets and info as edited), for building the server's document.
    pub fn editor_message(&self, text: &str, model_text: impl Fn(&Path) -> Option<String>) {
        let Ok(mut msg) = serde_json::from_str::<Json>(text) else {
            return;
        };
        let method = msg.get("method").and_then(Json::as_str).map(str::to_owned);
        if let (Some(id), Some(m)) = (msg.get("id"), &method) {
            self.pending
                .lock()
                .unwrap()
                .insert(id.to_string(), m.clone());
        }
        let uri = msg
            .pointer("/params/textDocument/uri")
            .and_then(Json::as_str)
            .map(str::to_owned);
        if let Some(uri) = uri
            && let Some(file_uri) = uri.strip_suffix(CODE_SUFFIX)
        {
            let file_uri = file_uri.to_string();
            match method.as_deref() {
                Some("textDocument/didOpen") | Some("textDocument/didChange") => {
                    let code = if method.as_deref() == Some("textDocument/didOpen") {
                        msg.pointer("/params/textDocument/text")
                            .and_then(Json::as_str)
                            .map(str::to_owned)
                    } else {
                        msg.pointer("/params/contentChanges")
                            .and_then(Json::as_array)
                            .and_then(|c| c.last())
                            .and_then(|c| c.get("text"))
                            .and_then(Json::as_str)
                            .map(str::to_owned)
                    };
                    let Some(code) = code else { return };
                    let Some(path) = file_path(&file_uri) else {
                        return;
                    };
                    let Some(base) = model_text(&path) else {
                        return;
                    };
                    let Some(doc) = ModelDoc::build(&path, file_uri.clone(), &base, &code) else {
                        return;
                    };
                    let full = doc.file_text.clone();
                    self.docs.lock().unwrap().insert(file_uri.clone(), doc);
                    if method.as_deref() == Some("textDocument/didOpen") {
                        msg["params"]["textDocument"]["text"] = Json::String(full);
                    } else {
                        msg["params"]["contentChanges"] = json!([{ "text": full }]);
                    }
                }
                Some("textDocument/didClose") => {
                    self.docs.lock().unwrap().remove(&file_uri);
                }
                _ => {
                    let docs = self.docs.lock().unwrap();
                    if let Some(doc) = docs.get(&file_uri) {
                        if let Some(p) = msg.pointer_mut("/params/position")
                            && let Some((l, c)) = pos(p)
                        {
                            let (l, c) = doc.to_file(l, c, false);
                            *p = json!({ "line": l, "character": c });
                        }
                        if let Some(Json::Array(ps)) = msg.pointer_mut("/params/positions") {
                            for p in ps.iter_mut() {
                                if let Some((l, c)) = pos(p) {
                                    let (l, c) = doc.to_file(l, c, false);
                                    *p = json!({ "line": l, "character": c });
                                }
                            }
                        }
                    }
                }
            }
            msg["params"]["textDocument"]["uri"] = Json::String(file_uri);
        }
        if let Ok(m) = serde_json::from_value::<Message>(msg) {
            let _ = self.to_server.send(m);
        }
    }

    /// Re-sends a model's code to the server after the host changed the file outside the code
    /// (for example a rename applied to widgets), so its positions stay right.
    pub fn refresh(&self, path: &Path, base: &str) {
        let Some(uri) = uri_of_path(path) else { return };
        let mut docs = self.docs.lock().unwrap();
        let Some(old) = docs.get(uri.as_str()) else {
            return;
        };
        let code = old.code.clone();
        let Some(doc) = ModelDoc::build(path, uri.as_str().to_string(), base, &code) else {
            return;
        };
        let full = doc.file_text.clone();
        docs.insert(uri.as_str().to_string(), doc);
        let note = lsp_server::Notification::new(
            "textDocument/didChange".into(),
            json!({
                "textDocument": { "uri": uri.as_str(), "version": 0x3fff_ffff },
                "contentChanges": [{ "text": full }],
            }),
        );
        let _ = self.to_server.send(Message::Notification(note));
    }

    /// The model doc whose file URI names the same file as `uri`.
    fn doc_key(docs: &HashMap<String, ModelDoc>, uri: &str) -> Option<String> {
        let parsed: lsp_types::Uri = uri.parse().ok()?;
        let key = key_of(&parsed);
        docs.iter()
            .find(|(_, d)| {
                d.file_uri
                    .parse::<lsp_types::Uri>()
                    .is_ok_and(|u| key_of(&u) == key)
            })
            .map(|(k, _)| k.clone())
    }

    /// A message from the server, as text for the editor; None to drop it.
    fn server_message(&self, msg: Message) -> Option<String> {
        let mut v = serde_json::to_value(&msg).ok()?;
        let docs = self.docs.lock().unwrap();
        match &msg {
            Message::Notification(n) if n.method == "textDocument/publishDiagnostics" => {
                let uri = v.pointer("/params/uri").and_then(Json::as_str)?.to_string();
                if let Some(k) = Self::doc_key(&docs, &uri) {
                    let doc = &docs[&k];
                    let diags = v.pointer("/params/diagnostics")?.as_array()?.clone();
                    let mapped: Vec<Json> = diags
                        .into_iter()
                        .filter_map(|mut d| {
                            let r = doc.range_to_code(d.get("range")?)?;
                            d["range"] = r;
                            Some(d)
                        })
                        .collect();
                    v["params"]["diagnostics"] = Json::Array(mapped);
                    v["params"]["uri"] = Json::String(format!("{k}{CODE_SUFFIX}"));
                }
            }
            Message::Response(r) => {
                let method = self
                    .pending
                    .lock()
                    .unwrap()
                    .remove(&serde_json::to_value(&r.id).ok()?.to_string());
                if let Some(result) = v.get_mut("result") {
                    let method = method.unwrap_or_default();
                    // Ranges without a URI belong to the document the request was about,
                    // which for these requests is the model's code.
                    let context = docs.keys().next().cloned();
                    if method == "textDocument/rename" {
                        drop(docs);
                        self.rename_result(result);
                        return serde_json::to_string(&v).ok();
                    }
                    let docs_ref = &docs;
                    map_value(result, docs_ref, context.as_deref());
                }
            }
            _ => {}
        }
        serde_json::to_string(&v).ok()
    }

    /// Splits a rename's edits: those in the code go to the editor, the rest are applied to
    /// the model document by the host.
    fn rename_result(&self, result: &mut Json) {
        let docs = self.docs.lock().unwrap();
        let mut outside: OutsideEdits = Vec::new();
        let mut model_path = None;
        if let Some(Json::Object(changes)) = result.get_mut("changes") {
            let keys: Vec<String> = changes.keys().cloned().collect();
            for uri in keys {
                let Some(k) = Self::doc_key(&docs, &uri) else {
                    continue;
                };
                let doc = &docs[&k];
                let edits = changes
                    .remove(&uri)
                    .and_then(|e| e.as_array().cloned())
                    .unwrap_or_default();
                let mut in_code = Vec::new();
                for e in edits {
                    let Some(range) = e.get("range") else {
                        continue;
                    };
                    let new_text = e.get("newText").and_then(Json::as_str).unwrap_or_default();
                    match doc.range_to_code(range) {
                        Some(r) => in_code.push(json!({ "range": r, "newText": new_text })),
                        None => {
                            if let Some(b) = doc.range_bytes(range) {
                                outside.push((doc.path.clone(), b, new_text.to_string()));
                            }
                        }
                    }
                }
                model_path = Some(doc.file_text.clone());
                changes.insert(format!("{k}{CODE_SUFFIX}"), Json::Array(in_code));
            }
        }
        drop(docs);
        if let Some(text) = model_path
            && !outside.is_empty()
        {
            (self.apply_outside)(outside, text);
        }
    }
}

fn file_path(file_uri: &str) -> Option<PathBuf> {
    url::Url::parse(file_uri).ok()?.to_file_path().ok()
}

/// Converts every range in a result from file to code positions, and the URIs of locations in
/// a model to its code URI. Ranges outside the code are removed with the object holding them.
fn map_value(v: &mut Json, docs: &HashMap<String, ModelDoc>, context: Option<&str>) -> bool {
    match v {
        Json::Array(items) => {
            items.retain_mut(|i| map_value(i, docs, context));
            true
        }
        Json::Object(o) => {
            let uri = o.get("uri").and_then(Json::as_str).map(str::to_owned);
            let target = o.get("targetUri").and_then(Json::as_str).map(str::to_owned);
            let here = uri.or(target.clone());
            let doc_key = match &here {
                Some(u) => LspProxy::doc_key(docs, u),
                None => context.map(str::to_owned),
            };
            if let (Some(_), Some(k)) = (&here, &doc_key) {
                let key = if o.contains_key("uri") {
                    "uri"
                } else {
                    "targetUri"
                };
                o.insert(key.into(), Json::String(format!("{k}{CODE_SUFFIX}")));
            }
            let doc = doc_key.as_deref().and_then(|k| docs.get(k));
            let range_keys: Vec<String> = o
                .iter()
                .filter(|(_, x)| is_range(x))
                .map(|(k, _)| k.clone())
                .collect();
            for k in range_keys {
                if let Some(doc) = doc {
                    match doc.range_to_code(&o[&k]) {
                        Some(r) => {
                            o.insert(k, r);
                        }
                        None => return false,
                    }
                }
            }
            let child_context = doc_key.as_deref().or(context);
            for (k, child) in o.iter_mut() {
                if is_range(child) || k == "uri" || k == "targetUri" {
                    continue;
                }
                if matches!(child, Json::Array(_) | Json::Object(_)) {
                    map_value(child, docs, child_context);
                }
            }
            true
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<model version=\"NetLogo 7.0.4\">\n  <code><![CDATA[globals [ a ]\nto go\n  set a a + 1\nend]]></code>\n  <widgets><monitor x=\"1\" y=\"1\" width=\"1\" height=\"1\" precision=\"3\" fontSize=\"11\">a</monitor></widgets>\n</model>\n";

    #[test]
    fn positions_round_trip_between_code_and_file() {
        let code = "globals [ a ]\nto go\n  set a a + 1\nend";
        let doc = ModelDoc::build(
            Path::new("/m/x.nlogox"),
            "file:///m/x.nlogox".into(),
            FILE,
            code,
        )
        .unwrap();
        // `go` on line 1 of the code.
        let (l, c) = doc.to_file(1, 3, false);
        assert_eq!(
            &doc.file_text[doc.file_index.offset(&doc.file_text, l, c)..][..2],
            "go"
        );
        assert_eq!(doc.to_code(l, c), Some((1, 3)));
        // The monitor's `a` is outside the code.
        let mon = doc.file_text.find(">a</monitor>").unwrap() + 1;
        let (ml, mc) = doc.file_index.position(&doc.file_text, mon);
        assert_eq!(doc.to_code(ml, mc), None);
    }

    #[test]
    fn maps_results_and_drops_outside_ranges() {
        let code = "globals [ a ]\nto go\n  set a a + 1\nend";
        let doc = ModelDoc::build(
            Path::new("/m/x.nlogox"),
            "file:///m/x.nlogox".into(),
            FILE,
            code,
        )
        .unwrap();
        let start = doc.file_text.find("set a").unwrap() + 4;
        let (l, c) = doc.file_index.position(&doc.file_text, start);
        let mon = doc.file_text.find(">a</monitor>").unwrap() + 1;
        let (ml, mc) = doc.file_index.position(&doc.file_text, mon);
        let mut docs = HashMap::new();
        docs.insert("file:///m/x.nlogox".to_string(), doc);
        let mut v = json!([
            { "uri": "file:///m/x.nlogox", "range": { "start": { "line": l, "character": c }, "end": { "line": l, "character": c + 1 } } },
            { "uri": "file:///m/x.nlogox", "range": { "start": { "line": ml, "character": mc }, "end": { "line": ml, "character": mc + 1 } } }
        ]);
        map_value(&mut v, &docs, None);
        let a = v.as_array().unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0]["uri"], "file:///m/x.nlogox#code");
        assert_eq!(a[0]["range"]["start"], json!({ "line": 2, "character": 6 }));
    }
}
