//! The language server (`logos lsp`, Part S). It speaks the Language Server Protocol over a
//! [`Connection`] (standard input and output for `logos lsp`) and works on `.nlogox` models and
//! `.nls` include files.
//!
//! Everything a request needs is computed on the main thread from the open documents and Logos's
//! parser. NetLogo is needed for two things, which a worker thread does through the bridge so
//! that requests never wait for it: the primitive table (D-01), and compile errors
//! (`workspace.check`, D-07). Diagnostics are published twice after an edit: the parser's syntax
//! errors at once, then NetLogo's compile errors when the check for that version of the code
//! returns. Checks for older versions are dropped.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::SystemTime;

use crossbeam_channel::{Receiver, Sender, select, unbounded};
use logos_bridge_client::{Bridge, BridgeConfig};
use logos_protocol::{ModelError, WorkspaceId};
use logos_syntax::Language;
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types as lsp;
use serde_json::Value;

use crate::analysis::{CompletionKind, Loc, SymbolKind};
use crate::diagnostics::{Origin, diagnostics};
use crate::program::{FileKey, Program, models_including, normalize};

// ------------------------------------------------------------------------------- the worker

enum Job {
    /// Load the primitive table for a set of extensions (sorted, lower case).
    Primitives(Vec<String>),
    /// Check a version of a model's code. The model file gives the interface globals, the
    /// extensions and the folder `__includes` files are read from.
    Check {
        root: FileKey,
        model: PathBuf,
        code: String,
        interface_globals: Vec<String>,
        generation: u64,
    },
}

enum Done {
    Primitives(Vec<String>, Result<Box<Language>, String>),
    Check {
        root: FileKey,
        generation: u64,
        result: Result<Vec<ModelError>, String>,
    },
    /// NetLogo isn't available; only the parser checks code.
    Unavailable(String),
}

/// The bridge configuration for checking code. NetLogo's bytecode generator is turned off:
/// it turns compiled procedures into JVM classes for speed, finds no errors, and takes most of
/// the time of a check (about three quarters for AA4). Models are never run in this bridge.
fn checking(config: &BridgeConfig) -> BridgeConfig {
    let mut c = config.clone();
    c.extra_jvm_args.push("-Dorg.nlogo.noGenerator=true".into());
    c
}

struct Opened {
    workspace: Option<WorkspaceId>,
    modified: Option<SystemTime>,
}

fn worker(config: Option<BridgeConfig>, jobs: Receiver<Job>, done: Sender<Done>) {
    let mut bridge: Option<Bridge> = None;
    let mut unavailable = false;
    let mut opened: HashMap<PathBuf, Opened> = HashMap::new();
    while let Ok(first) = jobs.recv() {
        // Take everything queued; keep only the newest check for each model.
        let mut primitives = Vec::new();
        let mut checks: Vec<Job> = Vec::new();
        for job in std::iter::once(first).chain(jobs.try_iter()) {
            match job {
                Job::Primitives(e) => primitives.push(e),
                Job::Check { ref root, .. } => {
                    let root = root.clone();
                    checks.retain(|c| !matches!(c, Job::Check { root: r, .. } if *r == root));
                    checks.push(job);
                }
            }
        }
        if bridge.is_none() && !unavailable {
            match &config {
                None => {
                    unavailable = true;
                    let _ = done.send(Done::Unavailable(
                        "NetLogo 7.0.4 or the Logos bridge was not found, so names in \
                         procedures aren't resolved and only Logos's parser checks the code"
                            .into(),
                    ));
                }
                Some(c) => match Bridge::spawn(&checking(c)) {
                    Ok(b) => bridge = Some(b),
                    Err(e) => {
                        unavailable = true;
                        let _ = done.send(Done::Unavailable(format!(
                            "NetLogo could not be started ({e}), so names in procedures \
                             aren't resolved and only Logos's parser checks the code"
                        )));
                    }
                },
            }
        }
        let Some(b) = &bridge else {
            for e in primitives {
                let _ = done.send(Done::Primitives(e, Err("NetLogo isn't available".into())));
            }
            continue;
        };
        for e in primitives {
            let names: Vec<&str> = e.iter().map(String::as_str).collect();
            let result = b
                .primitives(&names)
                .map_err(|err| err.to_string())
                .and_then(|t| Language::from_table(&t).map(Box::new));
            let _ = done.send(Done::Primitives(e, result));
        }
        for job in checks {
            let Job::Check {
                root,
                model,
                code,
                interface_globals,
                generation,
            } = job
            else {
                continue;
            };
            let modified = std::fs::metadata(&model).and_then(|m| m.modified()).ok();
            // Open the model once, and again after it is saved (it gives the extensions and the
            // folder include files are read from).
            let stale = opened.get(&model).is_none_or(|o| o.modified != modified);
            if stale {
                if let Some(Opened {
                    workspace: Some(ws),
                    ..
                }) = opened.remove(&model)
                {
                    let _ = b.dispose(ws);
                }
                let workspace = b.open(&model).ok().map(|r| r.workspace);
                opened.insert(
                    model.clone(),
                    Opened {
                        workspace,
                        modified,
                    },
                );
            }
            let result = match opened.get(&model).and_then(|o| o.workspace) {
                Some(ws) => b
                    .check_with(ws, &code, Some(&interface_globals))
                    .map(|r| r.errors)
                    .map_err(|e| e.to_string()),
                None => Err(format!("NetLogo couldn't open {}", model.display())),
            };
            let _ = done.send(Done::Check {
                root,
                generation,
                result,
            });
        }
    }
    if let Some(b) = bridge {
        let _ = b.shutdown();
    }
}

// ------------------------------------------------------------------------------- the server

struct OpenDoc {
    uri: lsp::Uri,
    version: i32,
    text: String,
}

struct Server<'c> {
    conn: &'c Connection,
    jobs: Sender<Job>,
    open: HashMap<FileKey, OpenDoc>,
    programs: HashMap<FileKey, Program>,
    languages: HashMap<Vec<String>, Arc<Language>>,
    requested: HashSet<Vec<String>>,
    netlogo_available: bool,
    generation: HashMap<FileKey, u64>,
    /// NetLogo's errors for each model, and the generation they are for.
    netlogo: HashMap<FileKey, (u64, Vec<ModelError>)>,
    /// The documents each program last published diagnostics for.
    published: HashMap<FileKey, Vec<FileKey>>,
}

/// Runs the server on `connection` until the client shuts it down. `bridge` is how to start
/// NetLogo; without it, the server works from Logos's parser alone.
pub fn serve(connection: Connection, bridge: Option<BridgeConfig>) -> Result<(), String> {
    let (id, _params) = connection
        .initialize_start()
        .map_err(|e| format!("initialization failed: {e}"))?;
    let result = lsp::InitializeResult {
        capabilities: capabilities(),
        server_info: Some(lsp::ServerInfo {
            name: "logos".into(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
        }),
    };
    connection
        .initialize_finish(id, serde_json::to_value(result).map_err(|e| e.to_string())?)
        .map_err(|e| format!("initialization failed: {e}"))?;

    let (jobs_tx, jobs_rx) = unbounded();
    let (done_tx, done_rx) = unbounded();
    let worker = std::thread::spawn(move || worker(bridge, jobs_rx, done_tx));
    let mut server = Server {
        conn: &connection,
        jobs: jobs_tx,
        open: HashMap::new(),
        programs: HashMap::new(),
        languages: HashMap::new(),
        requested: HashSet::new(),
        netlogo_available: true,
        generation: HashMap::new(),
        netlogo: HashMap::new(),
        published: HashMap::new(),
    };
    // Load the core table now, so it is ready by the time a model is opened.
    server.request_language(&[]);
    let outcome = server.run(&done_rx);
    drop(server);
    let _ = worker.join();
    outcome
}

fn capabilities() -> lsp::ServerCapabilities {
    lsp::ServerCapabilities {
        text_document_sync: Some(lsp::TextDocumentSyncCapability::Options(
            lsp::TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(lsp::TextDocumentSyncKind::FULL),
                save: Some(lsp::TextDocumentSyncSaveOptions::Supported(true)),
                ..Default::default()
            },
        )),
        document_symbol_provider: Some(lsp::OneOf::Left(true)),
        definition_provider: Some(lsp::OneOf::Left(true)),
        references_provider: Some(lsp::OneOf::Left(true)),
        hover_provider: Some(lsp::HoverProviderCapability::Simple(true)),
        completion_provider: Some(lsp::CompletionOptions::default()),
        selection_range_provider: Some(lsp::SelectionRangeProviderCapability::Simple(true)),
        rename_provider: Some(lsp::OneOf::Right(lsp::RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        ..Default::default()
    }
}

/// The server's name for a document.
pub fn key_of(uri: &lsp::Uri) -> FileKey {
    match url::Url::parse(uri.as_str()) {
        Ok(u) if u.scheme() == "file" => match u.to_file_path() {
            Ok(p) => FileKey::Path(normalize(&p)),
            Err(()) => FileKey::Other(uri.as_str().to_owned()),
        },
        _ => FileKey::Other(uri.as_str().to_owned()),
    }
}

/// A `file:` URI for a path.
pub fn uri_of_path(path: &Path) -> Option<lsp::Uri> {
    let u = url::Url::from_file_path(path).ok()?;
    lsp::Uri::from_str(u.as_str()).ok()
}

/// A request that fails: the error code and message.
struct Failed(ErrorCode, String);

impl Failed {
    fn new(message: impl Into<String>) -> Failed {
        Failed(ErrorCode::RequestFailed, message.into())
    }
}

type Handled = Result<Value, Failed>;

impl Server<'_> {
    fn run(&mut self, done: &Receiver<Done>) -> Result<(), String> {
        loop {
            select! {
                recv(self.conn.receiver) -> msg => {
                    let Ok(msg) = msg else { return Ok(()) };
                    match msg {
                        Message::Request(req) => {
                            if self.conn.handle_shutdown(&req).map_err(|e| e.to_string())? {
                                return Ok(());
                            }
                            self.request(req);
                        }
                        Message::Notification(n) => {
                            if n.method == "exit" {
                                return Ok(());
                            }
                            self.notification(n);
                        }
                        Message::Response(_) => {}
                    }
                }
                recv(done) -> d => {
                    if let Ok(d) = d {
                        self.done(d);
                    }
                }
            }
        }
    }

    fn send(&self, msg: Message) {
        let _ = self.conn.sender.send(msg);
    }

    fn notify<N: lsp::notification::Notification>(&self, params: N::Params) {
        self.send(Message::Notification(Notification::new(
            N::METHOD.to_owned(),
            params,
        )));
    }

    fn log(&self, typ: lsp::MessageType, message: String) {
        self.notify::<lsp::notification::LogMessage>(lsp::LogMessageParams { typ, message });
    }

    // ----------------------------------------------------------------- documents and programs

    fn read(&self, key: &FileKey) -> Option<String> {
        if let Some(d) = self.open.get(key) {
            return Some(d.text.clone());
        }
        let bytes = std::fs::read(key.path()?).ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn uri_of(&self, key: &FileKey) -> Option<lsp::Uri> {
        if let Some(d) = self.open.get(key) {
            return Some(d.uri.clone());
        }
        match key {
            FileKey::Path(p) => uri_of_path(p),
            FileKey::Other(u) => lsp::Uri::from_str(u).ok(),
        }
    }

    /// The root of the program a document belongs to.
    fn root_for(&self, key: &FileKey) -> FileKey {
        if key.is_model() {
            return key.clone();
        }
        let mut containing: Vec<&FileKey> = self
            .programs
            .values()
            .filter(|p| p.root != *key && p.member(key).is_some())
            .map(|p| &p.root)
            .collect();
        containing.sort();
        if let Some(r) = containing.first() {
            return (*r).clone();
        }
        if let Some(p) = key.path()
            && let Some(dir) = p.parent()
            && let Some(m) = models_including(dir, p).first()
        {
            return FileKey::Path(normalize(m));
        }
        key.clone()
    }

    fn request_language(&mut self, extensions: &[String]) {
        if self.requested.insert(extensions.to_vec()) {
            let _ = self.jobs.send(Job::Primitives(extensions.to_vec()));
        }
    }

    fn rebuild(&mut self, root: &FileKey) {
        let wanted: std::cell::RefCell<Option<Vec<String>>> = Default::default();
        let program = {
            let read = |k: &FileKey| self.read(k);
            let language = |e: &[String]| {
                let found = self.languages.get(e).cloned();
                if found.is_none() {
                    *wanted.borrow_mut() = Some(e.to_vec());
                }
                found
            };
            Program::build(root.clone(), &read, &language)
        };
        if let Some(e) = wanted.into_inner() {
            self.request_language(&e);
        }
        let generation = {
            let g = self.generation.entry(root.clone()).or_insert(0);
            *g += 1;
            *g
        };
        let check = match (root.path(), root.is_model(), self.netlogo_available) {
            (Some(path), true, true) if program.members[0].view.error.is_none() => {
                Some(Job::Check {
                    root: root.clone(),
                    model: path.to_owned(),
                    code: program.members[0].view.code.clone(),
                    interface_globals: program.interface_globals.clone(),
                    generation,
                })
            }
            _ => None,
        };
        self.programs.insert(root.clone(), program);
        self.publish(root);
        if let Some(job) = check {
            let _ = self.jobs.send(job);
        }
    }

    /// Rebuilds every program a changed document belongs to.
    fn changed(&mut self, key: &FileKey) {
        let mut roots: Vec<FileKey> = self
            .programs
            .values()
            .filter(|p| p.member(key).is_some())
            .map(|p| p.root.clone())
            .collect();
        if roots.is_empty() && self.open.contains_key(key) {
            roots.push(self.root_for(key));
        }
        for r in roots {
            self.rebuild(&r);
        }
    }

    /// Drops programs none of whose documents are open, clearing their diagnostics.
    fn forget_closed(&mut self) {
        let closed: Vec<FileKey> = self
            .programs
            .values()
            .filter(|p| !p.members.iter().any(|m| self.open.contains_key(&m.key)))
            .map(|p| p.root.clone())
            .collect();
        for root in closed {
            self.programs.remove(&root);
            self.netlogo.remove(&root);
            for k in self.published.remove(&root).unwrap_or_default() {
                if let Some(uri) = self.uri_of(&k) {
                    self.notify::<lsp::notification::PublishDiagnostics>(
                        lsp::PublishDiagnosticsParams {
                            uri,
                            diagnostics: Vec::new(),
                            version: None,
                        },
                    );
                }
            }
        }
    }

    fn publish(&mut self, root: &FileKey) {
        let Some(program) = self.programs.get(root) else {
            return;
        };
        let current = self.generation.get(root).copied().unwrap_or(0);
        let netlogo: &[ModelError] = match self.netlogo.get(root) {
            Some((g, errors)) if *g == current => errors,
            _ => &[],
        };
        let model_dir = root.path().and_then(Path::parent).map(Path::to_owned);
        let mut keys = Vec::new();
        for (i, m) in program.members.iter().enumerate() {
            let errors: Vec<ModelError> = if i == 0 {
                netlogo
                    .iter()
                    .filter(|e| e.span.file.is_none())
                    .cloned()
                    .collect()
            } else {
                // NetLogo reads include files from disk, so its errors fit the editor's copy
                // only while that is saved.
                let saved = m
                    .key
                    .path()
                    .and_then(|p| std::fs::read(p).ok())
                    .is_some_and(|b| String::from_utf8_lossy(&b) == m.text);
                netlogo
                    .iter()
                    .filter(|e| {
                        saved
                            && e.span.file.as_ref().is_some_and(|f| {
                                let p = Path::new(f);
                                let p = match &model_dir {
                                    Some(d) if p.is_relative() => d.join(p),
                                    _ => p.to_owned(),
                                };
                                Some(normalize(&p).as_path()) == m.key.path()
                            })
                    })
                    .map(|e| {
                        let mut e = e.clone();
                        e.span.file = None;
                        e
                    })
                    .collect()
            };
            let mut out: Vec<lsp::Diagnostic> = Vec::new();
            if let Some(err) = &m.view.error {
                out.push(lsp::Diagnostic {
                    range: lsp::Range::default(),
                    severity: Some(lsp::DiagnosticSeverity::ERROR),
                    source: Some("logos".into()),
                    message: err.clone(),
                    ..Default::default()
                });
            }
            for d in diagnostics(&m.view.code, &errors, program.analysis.syntax_errors(i)) {
                out.push(lsp::Diagnostic {
                    range: lsp_range(m.range(&d.range)),
                    severity: Some(lsp::DiagnosticSeverity::ERROR),
                    source: Some(
                        match d.origin {
                            Origin::NetLogo => "netlogo",
                            Origin::Logos => "logos",
                        }
                        .into(),
                    ),
                    message: d.message,
                    ..Default::default()
                });
            }
            keys.push(m.key.clone());
            if let Some(uri) = self.uri_of(&m.key) {
                self.notify::<lsp::notification::PublishDiagnostics>(
                    lsp::PublishDiagnosticsParams {
                        uri,
                        diagnostics: out,
                        version: self.open.get(&m.key).map(|d| d.version),
                    },
                );
            }
        }
        let before = self.published.insert(root.clone(), keys.clone());
        for k in before.unwrap_or_default() {
            if !keys.contains(&k)
                && let Some(uri) = self.uri_of(&k)
            {
                self.notify::<lsp::notification::PublishDiagnostics>(
                    lsp::PublishDiagnosticsParams {
                        uri,
                        diagnostics: Vec::new(),
                        version: None,
                    },
                );
            }
        }
    }

    fn done(&mut self, d: Done) {
        match d {
            Done::Primitives(extensions, Ok(language)) => {
                self.languages
                    .insert(extensions.clone(), Arc::from(language));
                let roots: Vec<FileKey> = self
                    .programs
                    .values()
                    .filter(|p| p.extensions == extensions)
                    .map(|p| p.root.clone())
                    .collect();
                for r in roots {
                    self.rebuild(&r);
                }
            }
            Done::Primitives(extensions, Err(e)) => {
                if self.netlogo_available {
                    self.log(
                        lsp::MessageType::WARNING,
                        format!(
                            "NetLogo couldn't load the primitives for extensions [{}]: {e}",
                            extensions.join(" ")
                        ),
                    );
                }
            }
            Done::Check {
                root,
                generation,
                result,
            } => {
                if self.generation.get(&root) != Some(&generation) {
                    return;
                }
                match result {
                    Ok(errors) => {
                        self.netlogo.insert(root.clone(), (generation, errors));
                        self.publish(&root);
                    }
                    Err(e) => self.log(lsp::MessageType::WARNING, e),
                }
            }
            Done::Unavailable(message) => {
                self.netlogo_available = false;
                self.notify::<lsp::notification::ShowMessage>(lsp::ShowMessageParams {
                    typ: lsp::MessageType::WARNING,
                    message,
                });
            }
        }
    }

    // ------------------------------------------------------------------------ notifications

    fn notification(&mut self, n: Notification) {
        use lsp::notification::*;
        match n.method.as_str() {
            DidOpenTextDocument::METHOD => {
                if let Ok(p) = serde_json::from_value::<lsp::DidOpenTextDocumentParams>(n.params) {
                    let key = key_of(&p.text_document.uri);
                    self.open.insert(
                        key.clone(),
                        OpenDoc {
                            uri: p.text_document.uri,
                            version: p.text_document.version,
                            text: p.text_document.text,
                        },
                    );
                    self.changed(&key);
                }
            }
            DidChangeTextDocument::METHOD => {
                if let Ok(p) = serde_json::from_value::<lsp::DidChangeTextDocumentParams>(n.params)
                {
                    let key = key_of(&p.text_document.uri);
                    if let (Some(doc), Some(change)) = (
                        self.open.get_mut(&key),
                        p.content_changes.into_iter().last(),
                    ) {
                        doc.text = change.text;
                        doc.version = p.text_document.version;
                        self.changed(&key);
                    }
                }
            }
            DidSaveTextDocument::METHOD => {
                if let Ok(p) = serde_json::from_value::<lsp::DidSaveTextDocumentParams>(n.params) {
                    self.changed(&key_of(&p.text_document.uri));
                }
            }
            DidCloseTextDocument::METHOD => {
                if let Ok(p) = serde_json::from_value::<lsp::DidCloseTextDocumentParams>(n.params) {
                    let key = key_of(&p.text_document.uri);
                    self.open.remove(&key);
                    self.forget_closed();
                    // Programs still open now read the file from disk.
                    self.changed(&key);
                }
            }
            _ => {}
        }
    }

    // ----------------------------------------------------------------------------- requests

    fn request(&mut self, req: Request) {
        let id = req.id.clone();
        let result = self.handle(req);
        let response = match result {
            Ok(v) => Response::new_ok(id, v),
            Err(Failed(code, message)) => Response::new_err(id, code as i32, message),
        };
        self.send(Message::Response(response));
    }

    /// The program and member a document position is in, and the code offset.
    fn locate(&mut self, doc: &lsp::TextDocumentPositionParams) -> Option<(FileKey, usize, usize)> {
        let (root, i) = self.member_of(&doc.text_document.uri)?;
        let m = &self.programs[&root].members[i];
        let offset = m.offset(doc.position.line, doc.position.character);
        Some((root, i, offset))
    }

    fn member_of(&mut self, uri: &lsp::Uri) -> Option<(FileKey, usize)> {
        let key = key_of(uri);
        let root = self.root_for(&key);
        if !self.programs.contains_key(&root) {
            self.rebuild(&root);
        }
        let i = self.programs.get(&root)?.member(&key)?;
        Some((root, i))
    }

    fn location(&self, root: &FileKey, loc: &Loc) -> Option<lsp::Location> {
        let m = &self.programs[root].members[loc.source];
        Some(lsp::Location {
            uri: self.uri_of(&m.key)?,
            range: lsp_range(m.range(&loc.range)),
        })
    }

    // `WorkspaceEdit` keys its changes by `Uri`, which clippy flags as a mutable key type.
    #[allow(clippy::mutable_key_type)]
    fn handle(&mut self, req: Request) -> Handled {
        use lsp::request::*;
        fn params<P: serde::de::DeserializeOwned>(v: Value) -> Result<P, Failed> {
            serde_json::from_value(v)
                .map_err(|e| Failed(ErrorCode::InvalidParams, format!("invalid parameters: {e}")))
        }
        fn json<T: serde::Serialize>(v: T) -> Handled {
            serde_json::to_value(v).map_err(|e| Failed::new(e.to_string()))
        }
        match req.method.as_str() {
            DocumentSymbolRequest::METHOD => {
                let p: lsp::DocumentSymbolParams = params(req.params)?;
                let Some((root, i)) = self.member_of(&p.text_document.uri) else {
                    return json(Value::Null);
                };
                let program = &self.programs[&root];
                let m = &program.members[i];
                #[allow(deprecated)]
                let symbols: Vec<lsp::DocumentSymbol> = program
                    .analysis
                    .outline(i)
                    .into_iter()
                    .map(|s| lsp::DocumentSymbol {
                        name: s.name,
                        detail: s.detail,
                        kind: match s.kind {
                            SymbolKind::Command => lsp::SymbolKind::FUNCTION,
                            SymbolKind::Reporter => lsp::SymbolKind::FUNCTION,
                            SymbolKind::Global => lsp::SymbolKind::VARIABLE,
                            SymbolKind::Breed => lsp::SymbolKind::CLASS,
                            SymbolKind::Variable => lsp::SymbolKind::FIELD,
                            SymbolKind::Extension => lsp::SymbolKind::MODULE,
                        },
                        tags: None,
                        deprecated: None,
                        range: lsp_range(m.range(&s.range)),
                        selection_range: lsp_range(m.range(&s.name_range)),
                        children: None,
                    })
                    .collect();
                json(lsp::DocumentSymbolResponse::Nested(symbols))
            }
            GotoDefinition::METHOD => {
                let p: lsp::GotoDefinitionParams = params(req.params)?;
                let Some((root, i, offset)) = self.locate(&p.text_document_position_params) else {
                    return json(Value::Null);
                };
                let locs: Vec<lsp::Location> = self.programs[&root]
                    .analysis
                    .definition(i, offset)
                    .iter()
                    .filter_map(|l| self.location(&root, l))
                    .collect();
                json(lsp::GotoDefinitionResponse::Array(locs))
            }
            References::METHOD => {
                let p: lsp::ReferenceParams = params(req.params)?;
                let Some((root, i, offset)) = self.locate(&p.text_document_position) else {
                    return json(Value::Null);
                };
                let locs: Vec<lsp::Location> = self.programs[&root]
                    .analysis
                    .references(i, offset, p.context.include_declaration)
                    .iter()
                    .filter_map(|l| self.location(&root, l))
                    .collect();
                json(locs)
            }
            HoverRequest::METHOD => {
                let p: lsp::HoverParams = params(req.params)?;
                let Some((root, i, offset)) = self.locate(&p.text_document_position_params) else {
                    return json(Value::Null);
                };
                let program = &self.programs[&root];
                match program.analysis.hover(i, offset) {
                    None => json(Value::Null),
                    Some((range, text)) => json(lsp::Hover {
                        contents: lsp::HoverContents::Markup(lsp::MarkupContent {
                            kind: lsp::MarkupKind::Markdown,
                            value: format!("```netlogo\n{text}\n```"),
                        }),
                        range: Some(lsp_range(program.members[i].range(&range))),
                    }),
                }
            }
            Completion::METHOD => {
                let p: lsp::CompletionParams = params(req.params)?;
                let Some((root, i, offset)) = self.locate(&p.text_document_position) else {
                    return json(Value::Null);
                };
                let items: Vec<lsp::CompletionItem> = self.programs[&root]
                    .analysis
                    .completions(i, offset)
                    .into_iter()
                    .map(|c| lsp::CompletionItem {
                        label: c.label,
                        kind: Some(match c.kind {
                            CompletionKind::Keyword => lsp::CompletionItemKind::KEYWORD,
                            CompletionKind::Command => lsp::CompletionItemKind::FUNCTION,
                            CompletionKind::Reporter => lsp::CompletionItemKind::FUNCTION,
                            CompletionKind::Variable => lsp::CompletionItemKind::VARIABLE,
                            CompletionKind::Constant => lsp::CompletionItemKind::CONSTANT,
                        }),
                        detail: c.detail,
                        ..Default::default()
                    })
                    .collect();
                json(lsp::CompletionResponse::Array(items))
            }
            SelectionRangeRequest::METHOD => {
                let p: lsp::SelectionRangeParams = params(req.params)?;
                let Some((root, i)) = self.member_of(&p.text_document.uri) else {
                    return json(Value::Null);
                };
                let program = &self.programs[&root];
                let m = &program.members[i];
                let out: Vec<lsp::SelectionRange> = p
                    .positions
                    .iter()
                    .map(|pos| {
                        let offset = m.offset(pos.line, pos.character);
                        let ranges = program.analysis.selection_ranges(i, offset);
                        let mut chain: Option<lsp::SelectionRange> = None;
                        for r in ranges.iter().rev() {
                            chain = Some(lsp::SelectionRange {
                                range: lsp_range(m.range(r)),
                                parent: chain.map(Box::new),
                            });
                        }
                        chain.unwrap_or(lsp::SelectionRange {
                            range: lsp::Range::new(*pos, *pos),
                            parent: None,
                        })
                    })
                    .collect();
                json(out)
            }
            PrepareRenameRequest::METHOD => {
                let p: lsp::TextDocumentPositionParams = params(req.params)?;
                let Some((root, i, offset)) = self.locate(&p) else {
                    return Err(Failed::new("there is no procedure or variable name here"));
                };
                let program = &self.programs[&root];
                let range = program
                    .analysis
                    .prepare_rename(i, offset)
                    .map_err(Failed::new)?;
                json(lsp::PrepareRenameResponse::Range(lsp_range(
                    program.members[i].range(&range),
                )))
            }
            Rename::METHOD => {
                let p: lsp::RenameParams = params(req.params)?;
                let Some((root, i, offset)) = self.locate(&p.text_document_position) else {
                    return Err(Failed::new("there is no procedure or variable name here"));
                };
                let program = &self.programs[&root];
                let locs = program
                    .analysis
                    .rename(i, offset, &p.new_name)
                    .map_err(Failed::new)?;
                let mut changes: HashMap<lsp::Uri, Vec<lsp::TextEdit>> = HashMap::new();
                for l in &locs {
                    let Some(loc) = self.location(&root, l) else {
                        continue;
                    };
                    changes.entry(loc.uri).or_default().push(lsp::TextEdit {
                        range: loc.range,
                        new_text: p.new_name.clone(),
                    });
                }
                // Uses in the model's widgets and experiments.
                let model = &program.members[0];
                if let Some(key) = program.analysis.key_at(i, offset)
                    && let Some(uri) = self.uri_of(&model.key)
                {
                    for r in program.uses_outside_code(&key) {
                        let (a, b) = (
                            model.lines.position(&model.text, r.start),
                            model.lines.position(&model.text, r.end),
                        );
                        changes.entry(uri.clone()).or_default().push(lsp::TextEdit {
                            range: lsp_range((a, b)),
                            new_text: p.new_name.clone(),
                        });
                    }
                }
                json(lsp::WorkspaceEdit {
                    changes: Some(changes),
                    ..Default::default()
                })
            }
            _ => Err(Failed(
                ErrorCode::MethodNotFound,
                format!("unsupported request {}", req.method),
            )),
        }
    }
}

fn lsp_range(((l1, c1), (l2, c2)): ((u32, u32), (u32, u32))) -> lsp::Range {
    lsp::Range::new(lsp::Position::new(l1, c1), lsp::Position::new(l2, c2))
}
