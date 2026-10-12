//! Open models and the run bridge behind them (design 2.5, Part M).
//!
//! One run bridge serves every open model (D-25). Each model has an interactive workspace. A
//! dispatcher thread is the only reader of the bridge's notifications and data frames, and
//! routes them to the model whose workspace they belong to.
//!
//! Until protocol 1 (Part B) lands, three things the design needs are done here on top of
//! protocol 0, and are replaced by bridge methods later:
//!
//! - Snapshots (G15): a `workspace.run` of nothing with `repeat: 0` sends the view and agent
//!   state as they are, without running an iteration or touching the random number generator.
//! - Writes during a run (G1): a forever button is driven by a runner thread. Sliders, switches,
//!   the Command Center and once-buttons queue their commands; the runner stops the bridge run
//!   after its current iteration, runs the queued commands, and starts it again. As with G1,
//!   writes take effect between two iterations, in arrival order, on the main generator.
//! - The speed slider (G2): below full speed the runner runs one iteration at a time with a
//!   delay between them.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use logos_bridge_client::{Bridge, ClientError};
use logos_nlogox::widgets::{AgentKind, WidgetData, netlogo_string};
use logos_nlogox::{Nlogox, Widget, WidgetValue};
use logos_protocol::{
    AgentFrame, DataFrame, NlValue, ObserveStream, RunStreams, StreamRate, ViewFrame, WorkspaceId,
};
use serde::Serialize;
use serde_json::{Value as Json, json};

use crate::values;

/// Sends an event to the front end: (event name, payload).
pub type Emit = Arc<dyn Fn(&str, Json) + Send + Sync>;
/// Sends a binary message (a view or agent frame) to a model's view.
pub type FrameSink = Arc<dyn Fn(Vec<u8>) + Send + Sync>;

/// How long to wait for a run that was asked to stop.
const STOP_TIMEOUT: Duration = Duration::from_secs(30);
/// Monitors are sent to the front end at most this often during a run.
const MONITOR_INTERVAL: Duration = Duration::from_millis(50);

/// An error shown to the user. Compile errors carry the span NetLogo reported.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<(u64, u64)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl AppError {
    pub fn new(message: impl Into<String>) -> AppError {
        AppError {
            message: message.into(),
            span: None,
            detail: None,
        }
    }
}

impl From<ClientError> for AppError {
    fn from(e: ClientError) -> AppError {
        match e {
            ClientError::Rpc(rpc) => {
                let span = rpc.data.as_ref().and_then(|d| {
                    let s = d.get("span")?;
                    Some((s.get("start")?.as_u64()?, s.get("end")?.as_u64()?))
                });
                let detail = rpc.data.as_ref().and_then(|d| {
                    let agent = d.get("agent").and_then(Json::as_str);
                    let procedure = d.get("procedure").and_then(Json::as_str);
                    match (agent, procedure) {
                        (None, None) => None,
                        (a, p) => Some(format!(
                            "{}{}",
                            a.map(|a| format!("error while {a} running "))
                                .unwrap_or_default(),
                            p.map(|p| format!("called by {p}")).unwrap_or_default()
                        )),
                    }
                });
                AppError {
                    message: rpc.message,
                    span,
                    detail,
                }
            }
            other => AppError::new(other.to_string()),
        }
    }
}

impl From<String> for AppError {
    fn from(s: String) -> AppError {
        AppError::new(s)
    }
}

pub type AppResult<T> = Result<T, AppError>;

/// A finished bridge run, as the dispatcher saw it.
#[derive(Debug, Clone)]
pub struct Finished {
    pub reason: String,
    pub ticks: Option<f64>,
    pub error: Option<Json>,
}

/// Run ends the dispatcher has seen, for whoever waits for them.
#[derive(Default)]
struct Finishes {
    done: Mutex<HashMap<(u64, u64), Finished>>,
    changed: Condvar,
}

impl Finishes {
    fn put(&self, ws: u64, run: u64, f: Finished) {
        self.done.lock().unwrap().insert((ws, run), f);
        self.changed.notify_all();
    }

    fn wait(&self, ws: u64, run: u64, timeout: Duration) -> Option<Finished> {
        let deadline = Instant::now() + timeout;
        let mut done = self.done.lock().unwrap();
        loop {
            if let Some(f) = done.remove(&(ws, run)) {
                return Some(f);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            done = self.changed.wait_timeout(done, left).unwrap().0;
        }
    }
}

/// What a forever button runs.
#[derive(Debug, Clone)]
struct ForeverRun {
    button: Option<usize>,
}

/// The runner's shared state.
#[derive(Default)]
struct RunControl {
    /// The forever run in progress, if any.
    active: Option<ForeverRun>,
    /// The bridge run the runner is waiting on.
    bridge_run: Option<u64>,
    /// Commands to run between two iterations (G1).
    pending: VecDeque<String>,
    stop_requested: bool,
}

/// The document side of a model: the file as edited, and what is saved.
struct Doc {
    nlogox: Nlogox,
    saved_text: String,
    /// The code the interactive workspace was last compiled with.
    applied_code: String,
    /// File texts before each edit (code, info, interface, experiments), for one undo history
    /// per model (I3); most recent last.
    history: crate::history::History,
    /// The file's modification time and size when it was last read or written, to notice
    /// changes made by other programs.
    disk: Option<(SystemTime, u64)>,
}

fn disk_stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}

/// A model file changed on disk.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskChange {
    pub id: u64,
    /// True: the model had no unsaved edits and was reloaded. False: it has unsaved edits,
    /// and the user chooses which to keep.
    pub reloaded: bool,
    /// The file is gone (deleted or moved).
    pub missing: bool,
}

pub struct Session {
    pub id: u64,
    pub path: PathBuf,
    workspace: Mutex<WorkspaceId>,
    doc: Mutex<Doc>,
    frames: Mutex<Option<FrameSink>>,
    run: Mutex<RunControl>,
    /// Serializes commands, snapshots and run starts for this model.
    ops: Mutex<()>,
    /// 0..=100; 100 is full speed.
    speed: AtomicU64,
    /// Whether the view's frames are sent per iteration or only when ticks change.
    on_ticks: AtomicBool,
    last_monitors: Mutex<Instant>,
    last_ticks: Mutex<Option<f64>>,
}

/// One monitor's value.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorValue {
    pub index: usize,
    pub text: String,
    pub error: bool,
}

/// What the front end needs to show an open model.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelView {
    pub id: u64,
    pub path: PathBuf,
    pub name: String,
    pub version: Option<String>,
    pub widgets: Vec<Widget>,
    pub code: String,
    pub info: String,
    pub compiled: bool,
    pub errors: Vec<AppError>,
    pub dirty: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineEntry {
    pub name: String,
    pub kind: String,
    pub detail: Option<String>,
    /// Byte offsets in the code.
    pub start: usize,
    pub end: usize,
    pub line: usize,
}

pub struct Sessions {
    bridge: Arc<Bridge>,
    emit: Emit,
    models: Mutex<HashMap<u64, Arc<Session>>>,
    finishes: Arc<Finishes>,
    next_id: AtomicU64,
    quit: Arc<AtomicBool>,
}

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

/// Common header of a binary message to the front end, 32 bytes: kind (u8), 7 bytes padding,
/// sequence (u64), ticks (f64, NaN before `reset-ticks`), time sent (f64, ms since the epoch).
fn header(out: &mut Vec<u8>, kind: u8, sequence: u64, ticks: Option<f64>) {
    out.push(kind);
    out.extend_from_slice(&[0; 7]);
    out.extend_from_slice(&sequence.to_le_bytes());
    out.extend_from_slice(&ticks.unwrap_or(f64::NAN).to_le_bytes());
    out.extend_from_slice(&now_ms().to_le_bytes());
}

/// A view frame: header, width (u32), height (u32), RGBA.
pub fn encode_view(v: &ViewFrame) -> Vec<u8> {
    let mut out = Vec::with_capacity(40 + v.rgba.len());
    header(&mut out, 1, v.sequence, v.ticks);
    out.extend_from_slice(&v.width.to_le_bytes());
    out.extend_from_slice(&v.height.to_le_bytes());
    out.extend_from_slice(&v.rgba);
    out
}

/// Agent state, laid out for typed-array views in the front end (as in spike F1): header;
/// min/max pxcor/pycor (4 × i32), turtle count (u32), padding; patch RGBA padded to 8; who,
/// xcor, ycor, heading, size (f64 columns); color (u32), padded to 8; shape, breed (u16);
/// hidden (u8).
pub fn encode_agents(a: &AgentFrame) -> Vec<u8> {
    let n = a.turtles.len();
    let mut out = Vec::with_capacity(64 + a.patch_rgba.len() + n * 53 + 16);
    header(&mut out, 2, a.sequence, a.ticks);
    for v in [a.min_pxcor, a.max_pxcor, a.min_pycor, a.max_pycor] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&(n as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&a.patch_rgba);
    while !out.len().is_multiple_of(8) {
        out.push(0);
    }
    let t = &a.turtles;
    for col in [&t.who, &t.xcor, &t.ycor, &t.heading, &t.size] {
        for v in col {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    for c in &t.color {
        out.extend_from_slice(c);
    }
    while !out.len().is_multiple_of(8) {
        out.push(0);
    }
    for v in &t.shape {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for v in &t.breed {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend(t.hidden.iter().map(|&h| u8::from(h)));
    out
}

/// Wraps commands for an agent kind, as NetLogo does for buttons and the Command Center.
pub fn for_agents(kind: &str, source: &str) -> String {
    match kind {
        "turtles" | "Turtle" => format!("ask turtles [\n{source}\n]"),
        "patches" | "Patch" => format!("ask patches [\n{source}\n]"),
        "links" | "Link" => format!("ask links [\n{source}\n]"),
        _ => source.to_string(),
    }
}

/// The `set` command that gives a widget's variable a value.
pub fn set_command(data: &WidgetData, value: &WidgetValue) -> Option<String> {
    let variable = data.variable()?;
    let literal = match (data, value) {
        (WidgetData::Slider { .. }, WidgetValue::Number(v)) => values::number(*v),
        (WidgetData::Switch { .. }, WidgetValue::Bool(b)) => b.to_string(),
        (WidgetData::Chooser { choices, .. }, WidgetValue::Choice(i)) => {
            choices.get(*i)?.code.clone()
        }
        (WidgetData::Input { input_type, .. }, WidgetValue::Text(t)) => match input_type.as_str() {
            "number" | "color" => {
                let t = t.trim();
                t.parse::<f64>().ok()?;
                t.to_string()
            }
            _ => netlogo_string(t),
        },
        _ => return None,
    };
    Some(format!("set {variable} {literal}"))
}

impl Session {
    pub fn workspace(&self) -> WorkspaceId {
        *self.workspace.lock().unwrap()
    }

    fn widgets(&self) -> Vec<Widget> {
        self.doc.lock().unwrap().nlogox.typed_widgets()
    }

    /// (widget index, reporter, precision) of every monitor with a reporter.
    fn monitors(&self) -> Vec<(usize, String, i32)> {
        self.widgets()
            .into_iter()
            .filter_map(|w| match w.data {
                WidgetData::Monitor {
                    source, precision, ..
                } if !source.trim().is_empty() => Some((w.index, source, precision)),
                _ => None,
            })
            .collect()
    }

    fn send_frame(&self, bytes: Vec<u8>) {
        let sink = self.frames.lock().unwrap().clone();
        if let Some(sink) = sink {
            sink(bytes);
        }
    }

    pub fn view(
        &self,
        compiled: bool,
        errors: Vec<AppError>,
        version: Option<String>,
    ) -> ModelView {
        let doc = self.doc.lock().unwrap();
        ModelView {
            id: self.id,
            path: self.path.clone(),
            name: self
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            version,
            widgets: doc.nlogox.typed_widgets(),
            code: doc.nlogox.code().unwrap_or_default(),
            info: doc.nlogox.info().unwrap_or_default(),
            compiled,
            errors,
            dirty: doc.nlogox.text() != doc.saved_text,
        }
    }

    pub fn is_dirty(&self) -> bool {
        let doc = self.doc.lock().unwrap();
        doc.nlogox.text() != doc.saved_text
    }

    pub fn code_changed_since_apply(&self) -> bool {
        let doc = self.doc.lock().unwrap();
        doc.nlogox.code().unwrap_or_default() != doc.applied_code
    }
}

impl Sessions {
    pub fn new(bridge: Arc<Bridge>, emit: Emit) -> Arc<Sessions> {
        let s = Arc::new(Sessions {
            bridge,
            emit,
            models: Mutex::new(HashMap::new()),
            finishes: Arc::new(Finishes::default()),
            next_id: AtomicU64::new(1),
            quit: Arc::new(AtomicBool::new(false)),
        });
        let d = s.clone();
        std::thread::Builder::new()
            .name("logos-dispatch".into())
            .spawn(move || d.dispatch())
            .expect("start the dispatcher thread");
        s
    }

    pub fn bridge(&self) -> &Arc<Bridge> {
        &self.bridge
    }

    pub fn shutdown(&self) {
        self.quit.store(true, Ordering::SeqCst);
        let models: Vec<Arc<Session>> = self.models.lock().unwrap().values().cloned().collect();
        for m in models {
            let _ = self.stop(m.id);
        }
    }

    pub fn get(&self, id: u64) -> AppResult<Arc<Session>> {
        self.models
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| AppError::new(format!("no open model {id}")))
    }

    fn by_workspace(&self, ws: u64) -> Option<Arc<Session>> {
        self.models
            .lock()
            .unwrap()
            .values()
            .find(|s| s.workspace().0 == ws)
            .cloned()
    }

    // ---------------------------------------------------------------------- the dispatcher

    fn dispatch(self: Arc<Self>) {
        while !self.quit.load(Ordering::SeqCst) {
            if let Some(frame) = self.bridge.next_data_frame(Duration::from_millis(15)) {
                let (ws, bytes) = match &frame {
                    DataFrame::View(v) => (v.workspace, encode_view(v)),
                    DataFrame::Agents(a) => (a.workspace, encode_agents(a)),
                };
                if let Some(s) = self.by_workspace(u64::from(ws)) {
                    if let DataFrame::View(v) = &frame {
                        *s.last_ticks.lock().unwrap() = v.ticks;
                    }
                    s.send_frame(bytes);
                }
            }
            while let Some(n) = self.bridge.try_notification() {
                self.notification(&n.method, &n.params);
            }
        }
    }

    fn notification(&self, method: &str, p: &Json) {
        let ws = p.get("workspace").and_then(Json::as_u64).unwrap_or(0);
        let session = self.by_workspace(ws);
        match method {
            "workspace.output" => {
                if let Some(s) = session {
                    let text = p.get("text").and_then(Json::as_str).unwrap_or_default();
                    let area = p.get("outputArea").and_then(Json::as_bool).unwrap_or(false);
                    (self.emit)(
                        "model-output",
                        json!({ "model": s.id, "text": text, "outputArea": area }),
                    );
                }
            }
            "run.observed" => {
                let Some(s) = session else { return };
                {
                    let mut last = s.last_monitors.lock().unwrap();
                    if last.elapsed() < MONITOR_INTERVAL {
                        return;
                    }
                    *last = Instant::now();
                }
                if let Ok(obs) = logos_protocol::ObserveResult::from_json(p) {
                    self.emit_monitors(&s, obs.ticks, &obs.values);
                }
            }
            "run.finished" => {
                let run = p.get("run").and_then(Json::as_u64).unwrap_or(0);
                let f = Finished {
                    reason: p
                        .get("reason")
                        .and_then(Json::as_str)
                        .unwrap_or("error")
                        .to_string(),
                    ticks: p.get("ticks").and_then(Json::as_f64),
                    error: p.get("error").filter(|e| !e.is_null()).cloned(),
                };
                self.finishes.put(ws, run, f);
            }
            _ => {}
        }
    }

    fn emit_monitors(
        &self,
        s: &Session,
        ticks: Option<f64>,
        values: &[Result<NlValue, logos_protocol::RpcError>],
    ) {
        let monitors = s.monitors();
        let out: Vec<MonitorValue> = monitors
            .iter()
            .zip(values)
            .map(|((index, _, precision), v)| match v {
                Ok(v) => MonitorValue {
                    index: *index,
                    text: values::display(v, Some(*precision)),
                    error: false,
                },
                Err(_) => MonitorValue {
                    index: *index,
                    text: "N/A".into(),
                    error: true,
                },
            })
            .collect();
        (self.emit)(
            "model-monitors",
            json!({ "model": s.id, "ticks": ticks, "values": out }),
        );
    }

    /// Reads every monitor once (D-14) and sends the values.
    fn refresh_monitors(&self, s: &Session) {
        let monitors = s.monitors();
        let reporters: Vec<&str> = monitors.iter().map(|(_, r, _)| r.as_str()).collect();
        let reporters = if reporters.is_empty() {
            vec!["ticks"]
        } else {
            reporters
        };
        if let Ok(obs) = self.bridge.observe(s.workspace(), &reporters) {
            *s.last_ticks.lock().unwrap() = obs.ticks;
            if !monitors.is_empty() {
                self.emit_monitors(s, obs.ticks, &obs.values);
            }
            (self.emit)("model-ticks", json!({ "model": s.id, "ticks": obs.ticks }));
        }
    }

    /// Sends the view and agent state as they are now (G15, see the module comment).
    fn snapshot(&self, s: &Session) {
        let streams = RunStreams {
            view: Some(StreamRate {
                every_ticks: 1,
                max_fps: 1000.0,
                update: Default::default(),
            }),
            agents: Some(StreamRate {
                every_ticks: 1,
                max_fps: 1000.0,
                update: Default::default(),
            }),
            observe: None,
        };
        let ws = s.workspace();
        if let Ok(run) = self.bridge.run(ws, "", Some(0), &streams) {
            self.finishes.wait(ws.0, run, STOP_TIMEOUT);
        }
    }

    // ------------------------------------------------------------------------ opening

    pub fn open(&self, path: &Path) -> AppResult<ModelView> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| AppError::new(format!("{}: {e}", path.display())))?;
        let nlogox = Nlogox::parse(text.clone()).map_err(|e| {
            AppError::new(format!("{} is not a readable model: {e}", path.display()))
        })?;
        let opened = self.bridge.open(path)?;
        let errors: Vec<AppError> = opened
            .errors
            .iter()
            .map(|e| AppError {
                message: e.message.clone(),
                span: Some((e.span.start, e.span.end)),
                detail: e.span.file.clone(),
            })
            .collect();
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let applied_code = nlogox.code().unwrap_or_default();
        let session = Arc::new(Session {
            id,
            path: path.to_path_buf(),
            workspace: Mutex::new(opened.workspace),
            doc: Mutex::new(Doc {
                nlogox,
                saved_text: text,
                applied_code,
                history: Default::default(),
                disk: disk_stamp(path),
            }),
            frames: Mutex::new(None),
            run: Mutex::new(RunControl::default()),
            ops: Mutex::new(()),
            speed: AtomicU64::new(100),
            on_ticks: AtomicBool::new(false),
            last_monitors: Mutex::new(Instant::now() - MONITOR_INTERVAL),
            last_ticks: Mutex::new(None),
        });
        self.models.lock().unwrap().insert(id, session.clone());
        Ok(session.view(opened.compiled, errors, opened.model_version))
    }

    pub fn attach_view(&self, id: u64, sink: FrameSink) -> AppResult<()> {
        let s = self.get(id)?;
        *s.frames.lock().unwrap() = Some(sink);
        let _ops = s.ops.lock().unwrap();
        if s.run.lock().unwrap().active.is_none() {
            self.snapshot(&s);
            self.refresh_monitors(&s);
        }
        Ok(())
    }

    pub fn close(&self, id: u64) -> AppResult<()> {
        let s = self.get(id)?;
        let _ = self.stop(id);
        self.models.lock().unwrap().remove(&id);
        let _ = self.bridge.dispose(s.workspace());
        Ok(())
    }

    // ------------------------------------------------------------------------ commands

    /// Runs commands now, or between two iterations when a forever button is running (G1).
    pub fn command(&self, id: u64, source: &str) -> AppResult<()> {
        let s = self.get(id)?;
        {
            let mut run = s.run.lock().unwrap();
            if run.active.is_some() {
                run.pending.push_back(source.to_string());
                drop(run);
                // The runner sees the run end, runs the queued commands and starts it again.
                let _ = self.bridge.stop(s.workspace());
                return Ok(());
            }
        }
        let _ops = s.ops.lock().unwrap();
        let result =
            self.bridge
                .command_with_timeout(s.workspace(), source, Duration::from_secs(3600));
        self.snapshot(&s);
        self.refresh_monitors(&s);
        result.map_err(AppError::from)
    }

    /// Presses a button: once-buttons run their commands; forever buttons start or stop.
    pub fn press_button(self: &Arc<Self>, id: u64, index: usize) -> AppResult<()> {
        let s = self.get(id)?;
        let w = s
            .widgets()
            .into_iter()
            .nth(index)
            .ok_or_else(|| AppError::new(format!("no widget {index}")))?;
        let WidgetData::Button {
            source,
            forever,
            agent_kind,
            ..
        } = w.data
        else {
            return Err(AppError::new(format!("widget {index} is not a button")));
        };
        let kind = match agent_kind {
            AgentKind::Observer => "observer",
            AgentKind::Turtle => "turtles",
            AgentKind::Patch => "patches",
            AgentKind::Link => "links",
        };
        if !forever {
            return self.command(id, &for_agents(kind, &source));
        }
        if agent_kind != AgentKind::Observer {
            return Err(AppError::new(
                "Forever buttons for turtles, patches and links come in a later version of Logos (D-26).",
            ));
        }
        let running = s.run.lock().unwrap().active.clone();
        match running {
            Some(r) if r.button == Some(index) => self.stop(id),
            Some(_) => Err(AppError::new(
                "Another forever button is running. Logos runs one at a time for now (D-26).",
            )),
            None => self.start_forever(&s, source, Some(index)),
        }
    }

    /// Runs the first observer forever button's commands once (the run strip's Step).
    pub fn step(&self, id: u64) -> AppResult<()> {
        let s = self.get(id)?;
        if s.run.lock().unwrap().active.is_some() {
            return Err(AppError::new("The model is running; pause it first."));
        }
        let source = s
            .widgets()
            .into_iter()
            .find_map(|w| match w.data {
                WidgetData::Button {
                    source,
                    forever: true,
                    agent_kind: AgentKind::Observer,
                    ..
                } => Some(source),
                _ => None,
            })
            .ok_or_else(|| AppError::new("The model has no forever button to step."))?;
        let _ops = s.ops.lock().unwrap();
        let ws = s.workspace();
        let run = self.bridge.run(ws, &source, Some(1), &self.streams(&s))?;
        let f = self.finishes.wait(ws.0, run, Duration::from_secs(3600));
        self.refresh_monitors(&s);
        if let Some(f) = f
            && f.reason == "error"
        {
            return Err(run_error(&f));
        }
        Ok(())
    }

    pub fn stop(&self, id: u64) -> AppResult<()> {
        let s = self.get(id)?;
        let mut run = s.run.lock().unwrap();
        if run.active.is_some() {
            run.stop_requested = true;
            drop(run);
            self.bridge.stop(s.workspace())?;
        }
        Ok(())
    }

    fn streams(&self, s: &Session) -> RunStreams {
        let monitors: Vec<String> = s.monitors().into_iter().map(|(_, r, _)| r).collect();
        RunStreams {
            view: Some(StreamRate {
                every_ticks: 1,
                max_fps: 30.0,
                update: Default::default(),
            }),
            agents: Some(StreamRate {
                every_ticks: 1,
                max_fps: 10.0,
                update: Default::default(),
            }),
            observe: (!monitors.is_empty()).then_some(ObserveStream {
                reporters: monitors,
                every_ticks: 1,
            }),
        }
    }

    fn start_forever(
        self: &Arc<Self>,
        s: &Arc<Session>,
        source: String,
        button: Option<usize>,
    ) -> AppResult<()> {
        {
            let mut run = s.run.lock().unwrap();
            if run.active.is_some() {
                return Err(AppError::new("The model is already running."));
            }
            *run = RunControl {
                active: Some(ForeverRun { button }),
                ..RunControl::default()
            };
        }
        // Compile errors are reported at once; the runner reports everything else.
        let first = {
            let _ops = s.ops.lock().unwrap();
            self.start_bridge_run(s, &source)
        };
        let first = match first {
            Ok(r) => r,
            Err(e) => {
                s.run.lock().unwrap().active = None;
                return Err(e);
            }
        };
        (self.emit)(
            "model-run",
            json!({ "model": s.id, "running": true, "button": button }),
        );
        let (me, s2) = (self.clone(), s.clone());
        std::thread::Builder::new()
            .name(format!("logos-runner-{}", s.id))
            .spawn(move || me.runner(s2, source, button, first))
            .map_err(|e| AppError::new(e.to_string()))?;
        Ok(())
    }

    /// Starts one bridge run of a forever button's commands: until it stops at full speed,
    /// one iteration below it.
    fn start_bridge_run(&self, s: &Session, source: &str) -> AppResult<u64> {
        let full = s.speed.load(Ordering::SeqCst) >= 100;
        let run = self.bridge.run(
            s.workspace(),
            source,
            if full { None } else { Some(1) },
            &self.streams(s),
        )?;
        s.run.lock().unwrap().bridge_run = Some(run);
        Ok(run)
    }

    fn runner(self: Arc<Self>, s: Arc<Session>, source: String, button: Option<usize>, first: u64) {
        let ws = s.workspace();
        let mut current = Some(first);
        let mut outcome: Option<Finished> = None;
        while let Some(run) = current.take() {
            let Some(f) = self
                .finishes
                .wait(ws.0, run, Duration::from_secs(24 * 3600))
            else {
                outcome = Some(Finished {
                    reason: "error".into(),
                    ticks: None,
                    error: Some(json!({ "message": "no answer from the bridge" })),
                });
                break;
            };
            let mut ops = s.ops.lock().unwrap();
            // Queued writes run between two iterations (G1).
            let pending: Vec<String> = s.run.lock().unwrap().pending.drain(..).collect();
            for cmd in pending {
                if let Err(e) = self.bridge.command(ws, &cmd) {
                    let e = AppError::from(e);
                    (self.emit)("model-error", json!({ "model": s.id, "error": e }));
                }
            }
            let ended_by_model = matches!(f.reason.as_str(), "stopped" | "error");
            if s.run.lock().unwrap().stop_requested || ended_by_model {
                outcome = Some(f);
                break;
            }
            // Below full speed, wait before the next iteration (G2).
            let speed = s.speed.load(Ordering::SeqCst);
            if speed < 100 && f.reason == "completed" {
                drop(ops);
                std::thread::sleep(delay_for(speed));
                if s.run.lock().unwrap().stop_requested {
                    outcome = Some(f);
                    break;
                }
                ops = s.ops.lock().unwrap();
            }
            // Halted for queued writes or a speed change, or one stepped iteration done: go on.
            match self.start_bridge_run(&s, &source) {
                Ok(r) => current = Some(r),
                Err(e) => {
                    (self.emit)("model-error", json!({ "model": s.id, "error": e }));
                    break;
                }
            }
            drop(ops);
        }
        {
            let mut run = s.run.lock().unwrap();
            *run = RunControl::default();
        }
        let error = outcome
            .as_ref()
            .filter(|f| f.reason == "error")
            .map(run_error);
        (self.emit)(
            "model-run",
            json!({
                "model": s.id,
                "running": false,
                "button": button,
                "reason": outcome.as_ref().map(|f| f.reason.clone()),
                "ticks": outcome.as_ref().and_then(|f| f.ticks),
                "error": error,
            }),
        );
        let _ops = s.ops.lock().unwrap();
        self.refresh_monitors(&s);
    }

    /// Changes the speed (0 to 100). A run at full speed switches to stepped iterations at its
    /// next iteration, and back.
    pub fn set_speed(&self, id: u64, speed: u64) -> AppResult<()> {
        let s = self.get(id)?;
        let old = s.speed.swap(speed.min(100), Ordering::SeqCst);
        let crossed = (old >= 100) != (speed >= 100);
        if crossed && s.run.lock().unwrap().active.is_some() {
            // The runner restarts the run in the other mode.
            let _ = self.bridge.stop(s.workspace());
        }
        Ok(())
    }

    pub fn set_view_updates(&self, id: u64, on_ticks: bool) -> AppResult<()> {
        let s = self.get(id)?;
        s.on_ticks.store(on_ticks, Ordering::SeqCst);
        Ok(())
    }

    /// Sets a slider, switch, chooser or input box: the variable in the model (G1 during a
    /// run), and the value stored in the file, as NetLogo saves it.
    pub fn set_value(&self, id: u64, index: usize, value: &WidgetValue) -> AppResult<()> {
        let s = self.get(id)?;
        let w = s
            .widgets()
            .into_iter()
            .nth(index)
            .ok_or_else(|| AppError::new(format!("no widget {index}")))?;
        let cmd = set_command(&w.data, value)
            .ok_or_else(|| AppError::new("that value doesn't fit this widget"))?;
        s.doc
            .lock()
            .unwrap()
            .nlogox
            .set_widget_value(index, value)
            .map_err(|e| AppError::new(e.to_string()))?;
        self.command(id, &cmd)
    }

    pub fn observe(&self, id: u64, reporters: &[String]) -> AppResult<Json> {
        let s = self.get(id)?;
        let refs: Vec<&str> = reporters.iter().map(String::as_str).collect();
        let obs = self.bridge.observe(s.workspace(), &refs)?;
        let values: Vec<Json> = obs
            .values
            .iter()
            .map(|v| match v {
                Ok(v) => json!({ "text": values::display(v, None), "ok": true }),
                Err(e) => json!({ "text": e.message, "ok": false }),
            })
            .collect();
        Ok(json!({ "ticks": obs.ticks, "values": values }))
    }

    // ---------------------------------------------------------------------- the document

    pub fn set_code(&self, id: u64, code: &str) -> AppResult<bool> {
        self.set_text(id, "code", code)
    }

    pub fn set_info(&self, id: u64, info: &str) -> AppResult<bool> {
        self.set_text(id, "info", info)
    }

    /// Sets the code or info text, recording an undo step unless this continues typing of
    /// the same kind.
    fn set_text(&self, id: u64, kind: &'static str, value: &str) -> AppResult<bool> {
        let s = self.get(id)?;
        {
            let mut doc = s.doc.lock().unwrap();
            let current = if kind == "code" {
                doc.nlogox.code()
            } else {
                doc.nlogox.info()
            }
            .unwrap_or_default();
            if current != value {
                let before = doc.nlogox.text().to_string();
                let r = if kind == "code" {
                    doc.nlogox.set_code(value)
                } else {
                    doc.nlogox.set_info(value)
                };
                r.map_err(|e| AppError::new(e.to_string()))?;
                doc.history.record(before, Some(kind), Instant::now());
            }
        }
        Ok(s.is_dirty())
    }

    /// Whether the model has steps to undo and redo.
    pub fn history(&self, id: u64) -> AppResult<(bool, bool)> {
        let s = self.get(id)?;
        let doc = s.doc.lock().unwrap();
        Ok((doc.history.can_undo(), doc.history.can_redo()))
    }

    pub fn save(&self, id: u64) -> AppResult<()> {
        let s = self.get(id)?;
        let mut doc = s.doc.lock().unwrap();
        let text = doc.nlogox.text().to_string();
        crate::settings::write_atomically(&s.path, text.as_bytes())
            .map_err(|e| AppError::new(format!("{}: {e}", s.path.display())))?;
        doc.saved_text = text;
        doc.disk = disk_stamp(&s.path);
        Ok(())
    }

    /// Every open model's path, current text, and whether it has unsaved edits; for autosave.
    pub fn texts(&self) -> Vec<(PathBuf, String, bool)> {
        let models: Vec<Arc<Session>> = self.models.lock().unwrap().values().cloned().collect();
        models
            .iter()
            .map(|s| {
                let doc = s.doc.lock().unwrap();
                let text = doc.nlogox.text().to_string();
                let dirty = text != doc.saved_text;
                (s.path.clone(), text, dirty)
            })
            .collect()
    }

    /// Looks for open models changed on disk by another program. A model without unsaved
    /// edits is reloaded; the others are reported once per change.
    pub fn disk_changes(&self) -> Vec<DiskChange> {
        let models: Vec<Arc<Session>> = self.models.lock().unwrap().values().cloned().collect();
        let mut out = Vec::new();
        for s in models {
            let stamp = disk_stamp(&s.path);
            let mut doc = s.doc.lock().unwrap();
            if stamp == doc.disk {
                continue;
            }
            doc.disk = stamp;
            let Some(_) = stamp else {
                out.push(DiskChange {
                    id: s.id,
                    reloaded: false,
                    missing: true,
                });
                continue;
            };
            let Ok(text) = std::fs::read_to_string(&s.path) else {
                continue;
            };
            if text == doc.saved_text {
                continue; // touched, or our own save
            }
            let dirty = doc.nlogox.text() != doc.saved_text;
            if !dirty {
                match Nlogox::parse(text.clone()) {
                    Ok(n) => {
                        doc.nlogox = n;
                        doc.saved_text = text;
                        doc.history.clear();
                    }
                    // Half-written by the other program, perhaps; look again next time.
                    Err(_) => {
                        doc.disk = None;
                        continue;
                    }
                }
            }
            out.push(DiskChange {
                id: s.id,
                reloaded: !dirty,
                missing: false,
            });
        }
        out
    }

    /// Replaces the model's text with the file on disk, dropping unsaved edits.
    pub fn reload(&self, id: u64) -> AppResult<ModelView> {
        let s = self.get(id)?;
        let text = std::fs::read_to_string(&s.path)
            .map_err(|e| AppError::new(format!("{}: {e}", s.path.display())))?;
        let n = Nlogox::parse(text.clone()).map_err(|e| AppError::new(e.to_string()))?;
        {
            let mut doc = s.doc.lock().unwrap();
            doc.nlogox = n;
            doc.saved_text = text;
            doc.disk = disk_stamp(&s.path);
            doc.history.clear();
        }
        Ok(s.view(true, Vec::new(), None))
    }

    /// Puts recovered text in place of the model's text, as unsaved edits.
    pub fn restore(&self, id: u64, text: String) -> AppResult<ModelView> {
        let s = self.get(id)?;
        let n = Nlogox::parse(text).map_err(|e| AppError::new(e.to_string()))?;
        {
            let mut doc = s.doc.lock().unwrap();
            // Restoring is one step that undo takes back.
            let before = doc.nlogox.text().to_string();
            doc.history.record(before, None, Instant::now());
            doc.nlogox = n;
        }
        Ok(s.view(true, Vec::new(), None))
    }

    /// Applies edited code to the interactive workspace. Until `workspace.recompile` (G5)
    /// exists, this saves the model and opens it again in a new workspace, which clears the
    /// world.
    pub fn apply(&self, id: u64) -> AppResult<ModelView> {
        let s = self.get(id)?;
        if s.run.lock().unwrap().active.is_some() {
            return Err(AppError::new(
                "Pause the model before applying code changes.",
            ));
        }
        self.save(id)?;
        let _ops = s.ops.lock().unwrap();
        let opened = self.bridge.open(&s.path)?;
        let old = std::mem::replace(&mut *s.workspace.lock().unwrap(), opened.workspace);
        let _ = self.bridge.dispose(old);
        {
            let mut doc = s.doc.lock().unwrap();
            doc.applied_code = doc.nlogox.code().unwrap_or_default();
        }
        let errors = opened
            .errors
            .iter()
            .map(|e| AppError {
                message: e.message.clone(),
                span: Some((e.span.start, e.span.end)),
                detail: e.span.file.clone(),
            })
            .collect();
        self.snapshot(&s);
        self.refresh_monitors(&s);
        Ok(s.view(opened.compiled, errors, opened.model_version))
    }

    /// Applies an interface edit (D-19: through span edits) and records it for undo. Returns
    /// the widgets and whether the widgets' global variables changed, in which case the model
    /// must be applied again before they exist in NetLogo.
    pub fn edit_widgets(
        &self,
        id: u64,
        edit: impl FnOnce(&mut Nlogox) -> Result<(), String>,
    ) -> AppResult<WidgetEdit> {
        let s = self.get(id)?;
        let mut doc = s.doc.lock().unwrap();
        let before = doc.nlogox.text().to_string();
        let globals_before = interface_globals(&doc.nlogox);
        if let Err(e) = edit(&mut doc.nlogox) {
            // A batch that fails part way leaves nothing changed.
            if doc.nlogox.text() != before {
                doc.nlogox = Nlogox::parse(before).map_err(|e| AppError::new(e.to_string()))?;
            }
            return Err(AppError::new(e));
        }
        if doc.nlogox.text() != before {
            doc.history.record(before, None, Instant::now());
        }
        Ok(widget_edit(&doc, globals_before))
    }

    /// Undoes or redoes the last interface edit.
    pub fn undo_redo(&self, id: u64, redo: bool) -> AppResult<WidgetEdit> {
        let s = self.get(id)?;
        let mut doc = s.doc.lock().unwrap();
        let globals_before = interface_globals(&doc.nlogox);
        let current = doc.nlogox.text().to_string();
        let next = if redo {
            doc.history.redo(current)
        } else {
            doc.history.undo(current)
        };
        if let Some(text) = next {
            // The whole file goes back: code, info, widgets and experiments (one history, I3).
            doc.nlogox = Nlogox::parse(text).map_err(|e| AppError::new(e.to_string()))?;
        }
        Ok(widget_edit(&doc, globals_before))
    }

    /// The file text of an open model, as edited.
    pub fn text_of(&self, path: &Path) -> Option<String> {
        let models = self.models.lock().unwrap();
        let s = models.values().find(|s| s.path == path)?;
        Some(s.doc.lock().unwrap().nlogox.text().to_string())
    }

    /// Applies edits a rename made outside the code (widgets, experiments) to an open model.
    /// `server_text` is the file the edits' offsets refer to; the model's own code is kept.
    pub fn apply_outside_edits(
        &self,
        path: &Path,
        server_text: &str,
        mut edits: Vec<(std::ops::Range<usize>, String)>,
    ) -> Option<(u64, String)> {
        let s = {
            let models = self.models.lock().unwrap();
            models.values().find(|s| s.path == path)?.clone()
        };
        edits.sort_by_key(|e| std::cmp::Reverse(e.0.start));
        let mut text = server_text.to_string();
        for (r, new) in edits {
            if r.end <= text.len() && text.is_char_boundary(r.start) && text.is_char_boundary(r.end)
            {
                text.replace_range(r, &new);
            }
        }
        let mut doc = s.doc.lock().unwrap();
        let code = doc.nlogox.code().unwrap_or_default();
        let mut n = Nlogox::parse(text).ok()?;
        n.set_code(&code).ok()?;
        doc.nlogox = n;
        Some((s.id, doc.nlogox.text().to_string()))
    }

    pub fn widgets_of(&self, id: u64) -> AppResult<Vec<Widget>> {
        Ok(self.get(id)?.widgets())
    }

    pub fn outline(&self, id: u64) -> AppResult<Vec<OutlineEntry>> {
        let s = self.get(id)?;
        let code = s.doc.lock().unwrap().nlogox.code().unwrap_or_default();
        Ok(outline(&code))
    }
}

/// The result of an interface edit, for the front end.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetEdit {
    pub widgets: Vec<Widget>,
    pub dirty: bool,
    /// The widgets now define different global variables than before the edit.
    pub globals_changed: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    /// The code and info after the edit, so the editors can follow an undo.
    pub code: String,
    pub info: String,
}

fn interface_globals(n: &Nlogox) -> Vec<String> {
    let mut v: Vec<String> = n
        .typed_widgets()
        .iter()
        .filter_map(|w| w.data.variable().map(str::to_ascii_lowercase))
        .collect();
    v.sort();
    v
}

fn widget_edit(doc: &Doc, globals_before: Vec<String>) -> WidgetEdit {
    WidgetEdit {
        widgets: doc.nlogox.typed_widgets(),
        dirty: doc.nlogox.text() != doc.saved_text,
        globals_changed: interface_globals(&doc.nlogox) != globals_before,
        can_undo: doc.history.can_undo(),
        can_redo: doc.history.can_redo(),
        code: doc.nlogox.code().unwrap_or_default(),
        info: doc.nlogox.info().unwrap_or_default(),
    }
}

/// Declarations and procedures of a model's code, for the side panel.
pub fn outline(code: &str) -> Vec<OutlineEntry> {
    use logos_lsp::analysis::{Analysis, Source, SymbolKind};
    let a = Analysis::new(
        vec![Source {
            name: "model".into(),
            code: code.to_string(),
        }],
        Vec::new(),
        None,
    );
    a.outline(0)
        .into_iter()
        .map(|s| OutlineEntry {
            line: code[..s.range.start.min(code.len())].matches('\n').count() + 1,
            name: s.name,
            kind: match s.kind {
                SymbolKind::Command => "procedure",
                SymbolKind::Reporter => "reporter",
                SymbolKind::Global => "globals",
                SymbolKind::Breed => "breed",
                SymbolKind::Variable => "variables",
                SymbolKind::Extension => "extensions",
            }
            .into(),
            detail: s.detail,
            start: s.range.start,
            end: s.range.end,
        })
        .collect()
}

fn run_error(f: &Finished) -> AppError {
    let e = f.error.clone().unwrap_or(Json::Null);
    let message = e
        .get("message")
        .and_then(Json::as_str)
        .unwrap_or("runtime error")
        .to_string();
    let agent = e.get("agent").and_then(Json::as_str);
    let procedure = e.get("procedure").and_then(Json::as_str);
    AppError {
        message,
        span: None,
        detail: match (agent, procedure) {
            (None, None) => None,
            (a, p) => Some(
                [
                    a.map(|a| format!("error while {a} running")),
                    p.map(|p| format!("called by procedure {p}")),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(", "),
            ),
        },
    }
}

/// The pause between iterations for a speed below full (0 is the slowest): up to about
/// half a second, falling off quickly toward full speed.
pub fn delay_for(speed: u64) -> Duration {
    let slow = (100 - speed.min(100)) as f64 / 100.0;
    Duration::from_millis((slow * slow * 500.0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_commands() {
        let slider = WidgetData::Slider {
            variable: "rate".into(),
            min: "0".into(),
            max: "1".into(),
            step: "0.1".into(),
            value: 0.5,
            units: None,
            vertical: false,
        };
        assert_eq!(
            set_command(&slider, &WidgetValue::Number(0.25)).unwrap(),
            "set rate 0.25"
        );
        assert_eq!(set_command(&slider, &WidgetValue::Bool(true)), None);
        let input = WidgetData::Input {
            variable: "name".into(),
            input_type: "string".into(),
            value: String::new(),
            multiline: false,
        };
        assert_eq!(
            set_command(&input, &WidgetValue::Text("a \"b\"".into())).unwrap(),
            "set name \"a \\\"b\\\"\""
        );
        let number = WidgetData::Input {
            variable: "n".into(),
            input_type: "number".into(),
            value: String::new(),
            multiline: false,
        };
        assert_eq!(set_command(&number, &WidgetValue::Text("x".into())), None);
        assert_eq!(
            set_command(&number, &WidgetValue::Text(" 3 ".into())).unwrap(),
            "set n 3"
        );
    }

    #[test]
    fn agent_wrapping() {
        assert_eq!(for_agents("observer", "fd 1"), "fd 1");
        assert_eq!(for_agents("Turtle", "fd 1"), "ask turtles [\nfd 1\n]");
    }

    #[test]
    fn delays() {
        assert_eq!(delay_for(100), Duration::ZERO);
        assert_eq!(delay_for(0), Duration::from_millis(500));
        assert!(delay_for(50) < delay_for(25));
    }

    #[test]
    fn outline_of_code() {
        let o = outline("globals [ a b ]\nto setup\nend\nto-report x [ y ]\n  report y\nend\n");
        let names: Vec<_> = o
            .iter()
            .map(|e| (e.name.as_str(), e.kind.as_str(), e.line))
            .collect();
        assert_eq!(
            names,
            vec![
                ("globals", "globals", 1),
                ("setup", "procedure", 2),
                ("x", "reporter", 4)
            ]
        );
    }
}
