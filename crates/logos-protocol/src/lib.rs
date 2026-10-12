//! The Logos bridge protocol, Rust side. The specification is `protocol/PROTOCOL.md`; this
//! crate and `bridge/` must change together with it.

pub mod binary;
pub mod experiment;
pub mod frame;
pub mod value;

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

pub use binary::{AgentFrame, DataFrame, DataFrameError, TurtleColumns, ViewFrame};
pub use experiment::{
    Cell, ExperimentCommandResult, LabError, LabRunEnd, LabRunResult, LoadedExperiment,
    Measurement, Setting, WorldDimensions,
};
pub use frame::{Frame, FrameError, Kind, read_frame, write_frame};
pub use value::NlValue;

/// Protocol version implemented by this crate (PROTOCOL.md header).
pub const PROTOCOL_VERSION: u64 = 1;

// ---------------------------------------------------------------- envelopes

/// A request from client to bridge.
#[derive(Debug, Serialize)]
pub struct Request<'a, P: Serialize> {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub method: &'a str,
    pub params: P,
}

impl<'a, P: Serialize> Request<'a, P> {
    pub fn new(id: u64, method: &'a str, params: P) -> Self {
        Request {
            jsonrpc: "2.0",
            id,
            method,
            params,
        }
    }
}

/// A JSON-RPC error object (PROTOCOL.md section 8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[error("{message} (code {code})")]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Json>,
}

/// Error codes from PROTOCOL.md section 8.
pub mod codes {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const NOT_INITIALIZED: i64 = 1000;
    pub const COMPILER_ERROR: i64 = 1001;
    pub const RUNTIME_ERROR: i64 = 1002;
    pub const NO_SUCH_WORKSPACE: i64 = 1003;
    pub const MODEL_OPEN_FAILED: i64 = 1004;
    pub const BUSY: i64 = 1005;
    pub const PROTOCOL_MISMATCH: i64 = 1006;
    pub const FILE_EXISTS: i64 = 1007;
    pub const INTERNAL: i64 = 1099;
}

/// A message from the bridge.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// Response to request `id` (None only for errors about unreadable requests).
    Response {
        id: Option<u64>,
        result: Result<Json, RpcError>,
    },
    Notification {
        method: String,
        params: Json,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("message is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("message is neither a response nor a notification: {0}")]
    Shape(String),
}

impl Incoming {
    pub fn parse(bytes: &[u8]) -> Result<Incoming, DecodeError> {
        let v: Json = serde_json::from_slice(bytes)?;
        let obj = v
            .as_object()
            .ok_or_else(|| DecodeError::Shape("not an object".into()))?;
        if let Some(method) = obj.get("method").and_then(Json::as_str)
            && !obj.contains_key("id")
        {
            return Ok(Incoming::Notification {
                method: method.to_owned(),
                params: obj.get("params").cloned().unwrap_or(Json::Null),
            });
        }
        let id = match obj.get("id") {
            Some(Json::Null) => None,
            Some(id) => Some(
                id.as_u64()
                    .ok_or_else(|| DecodeError::Shape(format!("bad id {id}")))?,
            ),
            None => return Err(DecodeError::Shape("no id and no method".into())),
        };
        let result = if let Some(err) = obj.get("error") {
            Err(serde_json::from_value(err.clone())?)
        } else if let Some(res) = obj.get("result") {
            Ok(res.clone())
        } else {
            return Err(DecodeError::Shape(
                "response without result or error".into(),
            ));
        };
        Ok(Incoming::Response { id, result })
    }
}

// ---------------------------------------------------------------- methods

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloParams<'a> {
    pub protocol_version: u64,
    pub client: &'a str,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloResult {
    pub protocol_version: u64,
    pub bridge_version: String,
    pub netlogo_version: Option<String>,
    pub java_version: String,
    pub java_vendor: String,
    pub extensions_dirs: Vec<String>,
    pub models_dir: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkspaceId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SourceSpan {
    pub start: u64,
    pub end: u64,
    pub file: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelError {
    pub message: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenResult {
    pub workspace: WorkspaceId,
    pub model_version: Option<String>,
    pub compiled: bool,
    pub errors: Vec<ModelError>,
}

#[derive(Debug, Serialize)]
pub struct OpenParams<'a> {
    pub path: &'a str,
}

#[derive(Debug, Serialize)]
pub struct CommandParams<'a> {
    pub workspace: WorkspaceId,
    pub source: &'a str,
    /// Frames to send after the commands when no run is active (G15).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<SnapshotSpec>,
}

/// Which frames a snapshot sends (protocol 1, G15).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct SnapshotSpec {
    pub view: bool,
    pub agents: bool,
}

/// Params of `workspace.snapshot`.
#[derive(Debug, Serialize)]
pub struct SnapshotParams {
    pub workspace: WorkspaceId,
    pub view: bool,
    pub agents: bool,
}

#[derive(Debug, Serialize)]
pub struct ObserveParams<'a> {
    pub workspace: WorkspaceId,
    pub reporters: &'a [&'a str],
}

#[derive(Debug, Serialize)]
pub struct WorkspaceParams {
    pub workspace: WorkspaceId,
}

/// Stream settings for `workspace.run` (PROTOCOL.md section 5.2).
#[derive(Debug, Clone, Default, Serialize)]
pub struct RunStreams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<StreamRate>,
    /// Agent-state frames (section 6.2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents: Option<StreamRate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observe: Option<ObserveStream>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamRate {
    pub every_ticks: u64,
    pub max_fps: f64,
    /// Per iteration, or only when the tick counter changed (NetLogo's "on ticks").
    #[serde(skip_serializing_if = "StreamUpdate::is_default")]
    pub update: StreamUpdate,
}

impl StreamRate {
    /// `every_ticks` 1 at `max_fps`, sent per iteration.
    pub fn new(max_fps: f64) -> StreamRate {
        StreamRate {
            every_ticks: 1,
            max_fps,
            update: StreamUpdate::Iteration,
        }
    }
}

/// When a stream sends frames (protocol 1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamUpdate {
    #[default]
    Iteration,
    Ticks,
}

impl StreamUpdate {
    fn is_default(&self) -> bool {
        *self == StreamUpdate::Iteration
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObserveStream {
    pub reporters: Vec<String>,
    pub every_ticks: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunParams<'a> {
    pub workspace: WorkspaceId,
    pub source: &'a str,
    pub repeat: Option<u64>,
    pub stream: &'a RunStreams,
    /// Pause after each iteration, in milliseconds (G2).
    #[serde(skip_serializing_if = "is_zero")]
    pub delay_millis: f64,
    /// NetLogo's speed slider position, -50 (slowest) to 50 (fastest); `None` runs unpaced
    /// (G2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    /// A reporter checked after each iteration; the run ends when it reports true (G6).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_when: Option<&'a str>,
    /// Who runs the command. Only the observer in Phase 2 (D-26).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents: Option<RunAgents>,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// The agents a run's command runs as (D-26): `{ "kind": "observer" }` until agent forever
/// buttons come.
#[derive(Debug, Clone, Serialize)]
pub struct RunAgents {
    pub kind: String,
}

/// Options of a run beyond its command and streams (protocol 1).
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub delay_millis: f64,
    /// NetLogo's speed slider, -50 to 50; `None` for no pacing.
    pub speed: Option<f64>,
    pub stop_when: Option<String>,
}

/// Changes to a run in progress (`workspace.runSettings`, G2). `None` leaves a setting as
/// it is.
#[derive(Debug, Clone, Default)]
pub struct RunSettings {
    pub delay_millis: Option<f64>,
    /// `Some(None)` stops pacing; `Some(Some(s))` paces as NetLogo's slider at `s`.
    pub speed: Option<Option<f64>>,
    /// `Some(None)` removes the stop condition.
    pub stop_when: Option<Option<String>>,
    /// `Some(None)` stops the stream.
    pub view: Option<Option<StreamRate>>,
    pub agents: Option<Option<StreamRate>>,
}

impl RunSettings {
    /// The `workspace.runSettings` params.
    pub fn params(&self, workspace: WorkspaceId) -> Json {
        let mut p = serde_json::json!({ "workspace": workspace });
        if let Some(d) = self.delay_millis {
            p["delayMillis"] = serde_json::json!(d);
        }
        if let Some(s) = self.speed {
            p["speed"] = serde_json::json!(s);
        }
        if let Some(w) = &self.stop_when {
            p["stopWhen"] = serde_json::json!(w);
        }
        if self.view.is_some() || self.agents.is_some() {
            let mut stream = serde_json::Map::new();
            if let Some(v) = self.view {
                stream.insert("view".into(), serde_json::to_value(v).unwrap_or_default());
            }
            if let Some(a) = self.agents {
                stream.insert("agents".into(), serde_json::to_value(a).unwrap_or_default());
            }
            p["stream"] = Json::Object(stream);
        }
        p
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunEnd {
    /// The command executed `stop`.
    Stopped,
    /// `repeat` iterations finished.
    Completed,
    /// Stopped by the client, dispose or shutdown.
    Halted,
    /// A runtime error.
    Error,
    /// The run's `stopWhen` reporter reported true (protocol 1, G6).
    Condition,
}

/// The `run.finished` notification.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunFinished {
    pub workspace: WorkspaceId,
    pub run: u64,
    pub reason: RunEnd,
    pub iterations: u64,
    pub ticks: Option<f64>,
    pub elapsed_millis: f64,
    pub error: Option<Json>,
}

/// Params of `engine.primitives`.
#[derive(Debug, Serialize)]
pub struct PrimitivesParams<'a> {
    pub extensions: &'a [&'a str],
}

/// Result of `engine.primitives`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrimitivesResult {
    pub netlogo_version: String,
    pub primitives: Vec<PrimitiveInfo>,
    /// NetLogo's built-in agent variables, in NetLogo's order.
    #[serde(default)]
    pub agent_variables: AgentVariables,
    /// Names the parser reads as literal values: `true`, `nobody`, `pi`, color names.
    #[serde(default)]
    pub constants: Vec<ConstantInfo>,
    /// `to`, `to-report`, `end`, `globals`, ... (any name ending in `-own` is one too).
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Primitives NetLogo derives from each declared breed. `name` is a pattern in which
    /// `<breeds>` stands for the plural and `<breed>` for the singular.
    #[serde(default)]
    pub breed_primitives: Vec<BreedPrimitiveInfo>,
    /// The type masks a parser needs (`commandBlock`, `optional`, ...) and the command and
    /// normal precedences, from NetLogo's `Syntax`.
    #[serde(default)]
    pub type_masks: std::collections::BTreeMap<String, i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct AgentVariables {
    pub observer: Vec<VariableInfo>,
    pub turtle: Vec<VariableInfo>,
    pub patch: Vec<VariableInfo>,
    pub link: Vec<VariableInfo>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct VariableInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: ArgType,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ConstantInfo {
    pub name: String,
    pub value: Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BreedKind {
    Turtle,
    DirectedLink,
    UndirectedLink,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BreedPrimitiveInfo {
    #[serde(flatten)]
    pub primitive: PrimitiveInfo,
    pub breed_kind: BreedKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrimitiveKind {
    Command,
    Reporter,
}

/// One primitive's syntax as NetLogo defines it (`org.nlogo.core.Syntax`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrimitiveInfo {
    /// Lower case, with the extension prefix: `fd`, `nw:generate-random`.
    pub name: String,
    pub kind: PrimitiveKind,
    /// The argument on the left of an infix reporter.
    pub left: Option<ArgType>,
    /// Arguments on the right, in order.
    pub args: Vec<ArgType>,
    pub returns: Option<ArgType>,
    pub default_arg_count: u32,
    pub minimum_arg_count: u32,
    pub variadic: bool,
    pub precedence: i32,
    pub is_right_associative: bool,
    /// Which agents may run it, as NetLogo writes it: `O` observer, `T` turtle, `P` patch,
    /// `L` link, `-` not allowed. For example `OTPL` or `-T--`.
    pub agent_class: String,
    pub block_agent_class: Option<String>,
    pub introduces_context: bool,
    /// Usable by name alone where an anonymous procedure is expected (`foreach xs print`).
    #[serde(default)]
    pub can_be_concise: bool,
    pub extension: Option<String>,
}

/// An argument or return type: NetLogo's type bit mask and its name for it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ArgType {
    pub name: String,
    pub mask: u32,
}

/// Params of `workspace.check`.
#[derive(Debug, Serialize)]
pub struct CheckParams<'a> {
    pub workspace: WorkspaceId,
    pub source: &'a str,
    /// The globals the model's widgets define; `None` uses the workspace's. A model that
    /// didn't compile when it was opened has none in the workspace.
    #[serde(rename = "interfaceGlobals", skip_serializing_if = "Option::is_none")]
    pub interface_globals: Option<&'a [String]>,
}

/// Result of `workspace.check`: NetLogo's compile errors for the code (at most one; NetLogo
/// stops at the first). Spans are in UTF-16 code units.
#[derive(Debug, Clone, Deserialize)]
pub struct CheckResult {
    pub errors: Vec<ModelError>,
}

/// Params of `workspace.recompile` (protocol 1, G5).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecompileParams<'a> {
    pub workspace: WorkspaceId,
    pub source: &'a str,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub dry_run: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface_globals: Option<&'a [String]>,
}

/// Result of `workspace.recompile`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecompileResult {
    pub applied: bool,
    pub world_cleared: bool,
    pub losses: Vec<Loss>,
}

/// Something the world loses when new code is installed: a breed (its agents die) or a
/// variable (its values are lost).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Loss {
    /// `breed`, `linkBreed`, `global`, `turtleVariable`, `patchVariable`, `linkVariable` or
    /// `breedVariable`.
    pub kind: String,
    /// Upper case, as NetLogo stores it.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breed: Option<String>,
    /// For a breed: how many agents die.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents: Option<u64>,
}

/// Result of `workspace.plots` (protocol 1, G4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlotsResult {
    pub plots: Vec<Plot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plot {
    pub name: String,
    pub x_min: f64,
    pub x_max: f64,
    pub y_min: f64,
    pub y_max: f64,
    pub auto_plot_x: bool,
    pub auto_plot_y: bool,
    pub legend: bool,
    pub pens: Vec<PlotPen>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlotPen {
    pub name: String,
    /// ARGB.
    pub color: u32,
    /// 0 line, 1 bar, 2 point.
    pub mode: u8,
    pub interval: f64,
    pub hidden: bool,
    pub in_legend: bool,
    pub temporary: bool,
    /// The points start from the pen's first: earlier ones the client has are gone.
    pub reset: bool,
    /// x, y, pen down, ARGB color.
    pub points: Vec<(f64, f64, bool, u32)>,
}

/// The `run.plots` notification: plot changes since the last plots sent (G4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunPlots {
    pub workspace: WorkspaceId,
    /// 0 after a command or snapshot.
    pub run: u64,
    pub ticks: Option<f64>,
    pub plots: Vec<PlotChange>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlotChange {
    pub name: String,
    pub x_min: f64,
    pub x_max: f64,
    pub y_min: f64,
    pub y_max: f64,
    pub pens: Vec<PlotPen>,
    pub removed_pens: Vec<String>,
}

/// The `experiment.progress` notification (protocol 1, G11).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentProgress {
    pub workspace: WorkspaceId,
    pub experiment: u64,
    pub run_number: u64,
    pub steps: u64,
    pub ticks: Option<f64>,
    pub elapsed_millis: f64,
}

/// Params of `workspace.references`.
#[derive(Debug, Serialize)]
pub struct ReferencesParams<'a> {
    pub workspace: WorkspaceId,
    pub source: &'a str,
}

/// Result of `workspace.references`: what NetLogo's front end resolves names to.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ReferencesResult {
    pub references: Vec<ResolvedReference>,
}

/// A procedure call or variable reference as NetLogo resolves it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ResolvedReference {
    /// Offsets in UTF-16 code units into the source (or the include file).
    pub start: usize,
    pub end: usize,
    /// The include file, or `None` for the source sent.
    pub file: Option<String>,
    /// `procedure`, `global`, `turtle`, `patch`, `link`, `turtle-or-link`, `breed`, `local`.
    pub kind: String,
    /// Lower case, as written.
    pub name: String,
}

/// Result of `workspace.program`: the compiled model's declarations and procedures as NetLogo
/// holds them, names in upper case, NetLogo's built-in variables left out.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramResult {
    /// Globals declared in the code.
    pub globals: Vec<String>,
    /// Globals defined by interface widgets (sliders, switches, ...).
    pub interface_globals: Vec<String>,
    pub turtles_own: Vec<String>,
    pub patches_own: Vec<String>,
    pub links_own: Vec<String>,
    /// Turtle breeds, then link breeds, each in declaration order.
    pub breeds: Vec<BreedInfo>,
    pub procedures: Vec<ProcedureInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BreedInfo {
    pub name: String,
    pub singular: String,
    pub is_link_breed: bool,
    pub is_directed: bool,
    pub owns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcedureInfo {
    pub name: String,
    pub is_reporter: bool,
    pub args: Vec<String>,
    /// The file the procedure is in: empty for the model's code, otherwise an `__includes` file.
    pub file: Option<String>,
}

/// Params of `model.convertLegacy`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertLegacyParams<'a> {
    pub source_path: &'a str,
    pub output_path: &'a str,
}

/// Result of `model.convertLegacy`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertLegacyResult {
    pub output_path: String,
    /// The version line of the source file, for example `NetLogo 5.0.3`.
    pub source_version: String,
    /// Problems NetLogo's auto-conversion reported. The model was converted as far as it could
    /// be; the GUI shows the same messages.
    pub warnings: Vec<String>,
}

/// Result of `workspace.observe`. Each entry is a value or the error for that reporter.
#[derive(Debug, Clone, PartialEq)]
pub struct ObserveResult {
    pub ticks: Option<f64>,
    pub values: Vec<Result<NlValue, RpcError>>,
}

impl ObserveResult {
    pub fn from_json(v: &Json) -> Result<ObserveResult, DecodeError> {
        let ticks = v.get("ticks").and_then(Json::as_f64);
        let entries = v
            .get("values")
            .and_then(Json::as_array)
            .ok_or_else(|| DecodeError::Shape("observe result has no values".into()))?;
        let mut values = Vec::with_capacity(entries.len());
        for e in entries {
            if let Some(ok) = e.get("ok") {
                values.push(Ok(NlValue::from_json(ok)
                    .map_err(|m| DecodeError::Shape(format!("bad value: {m}")))?));
            } else if let Some(err) = e.get("error") {
                let code = err.get("code").and_then(Json::as_i64).unwrap_or(0);
                let message = err
                    .get("message")
                    .and_then(Json::as_str)
                    .unwrap_or_default()
                    .to_owned();
                values.push(Err(RpcError {
                    code,
                    message,
                    data: Some(err.clone()),
                }));
            } else {
                return Err(DecodeError::Shape(format!("bad observe entry {e}")));
            }
        }
        Ok(ObserveResult { ticks, values })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_serializes_as_json_rpc() {
        let r = Request::new(
            7,
            "workspace.command",
            CommandParams {
                workspace: WorkspaceId(1),
                source: "setup",
                snapshot: None,
            },
        );
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"jsonrpc":"2.0","id":7,"method":"workspace.command",
                   "params":{"workspace":1,"source":"setup"}})
        );
    }

    #[test]
    fn parses_responses_errors_and_notifications() {
        let ok = Incoming::parse(br#"{"jsonrpc":"2.0","id":3,"result":{}}"#).unwrap();
        assert_eq!(
            ok,
            Incoming::Response {
                id: Some(3),
                result: Ok(json!({}))
            }
        );

        let err = Incoming::parse(
            br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"bad"}}"#,
        )
        .unwrap();
        let Incoming::Response {
            id: None,
            result: Err(e),
        } = err
        else {
            panic!("expected an error response")
        };
        assert_eq!(e.code, codes::PARSE_ERROR);

        let n = Incoming::parse(
            br#"{"jsonrpc":"2.0","method":"workspace.output","params":{"text":"hi"}}"#,
        )
        .unwrap();
        assert!(matches!(n, Incoming::Notification { method, .. } if method == "workspace.output"));

        assert!(Incoming::parse(b"[]").is_err());
        assert!(Incoming::parse(br#"{"jsonrpc":"2.0","id":1}"#).is_err());
    }

    #[test]
    fn protocol_one_messages() {
        let plots: PlotsResult = serde_json::from_value(json!({"plots":[{"name":"Noise",
            "xMin":0.0,"xMax":10.0,"yMin":0.0,"yMax":100.0,"autoPlotX":true,"autoPlotY":true,
            "legend":true,"pens":[{"name":"random","color":4291767081u32,"mode":0,
            "interval":1.0,"hidden":false,"inLegend":true,"temporary":false,"reset":true,
            "points":[[0.0,42.0,true,4291767081u32]]}]}]}))
        .unwrap();
        assert_eq!(
            plots.plots[0].pens[0].points[0],
            (0.0, 42.0, true, 4291767081)
        );
        let change: RunPlots = serde_json::from_value(json!({"workspace":1,"run":0,"ticks":null,
            "plots":[{"name":"Noise","xMin":0.0,"xMax":10.0,"yMin":0.0,"yMax":100.0,"pens":[],
            "removedPens":["temp"]}]}))
        .unwrap();
        assert_eq!(change.plots[0].removed_pens, ["temp"]);
        let r: RecompileResult = serde_json::from_value(json!({"applied":false,
            "worldCleared":false,"losses":[{"kind":"breed","name":"DOGS","agents":2},
            {"kind":"breedVariable","breed":"CATS","name":"AGE"}]}))
        .unwrap();
        assert_eq!(r.losses[0].agents, Some(2));
        assert_eq!(r.losses[1].breed.as_deref(), Some("CATS"));
        let p = serde_json::to_value(RecompileParams {
            workspace: WorkspaceId(1),
            source: "to go end",
            dry_run: true,
            interface_globals: None,
        })
        .unwrap();
        assert_eq!(p, json!({"workspace":1,"source":"to go end","dryRun":true}));
        let settings = RunSettings {
            speed: Some(None),
            stop_when: Some(Some("ticks > 5".into())),
            view: Some(None),
            ..RunSettings::default()
        };
        assert_eq!(
            settings.params(WorkspaceId(2)),
            json!({"workspace":2,"speed":null,"stopWhen":"ticks > 5","stream":{"view":null}})
        );
        let progress: ExperimentProgress = serde_json::from_value(json!({"workspace":1,
            "experiment":3,"runNumber":2,"steps":40,"ticks":40.0,"elapsedMillis":251.5}))
        .unwrap();
        assert_eq!((progress.run_number, progress.steps), (2, 40));
    }

    #[test]
    fn run_params_and_finished_notification() {
        let streams = RunStreams {
            view: Some(StreamRate {
                every_ticks: 1,
                max_fps: 30.0,
                update: Default::default(),
            }),
            agents: Some(StreamRate {
                every_ticks: 2,
                max_fps: 10.0,
                update: Default::default(),
            }),
            observe: None,
        };
        let p = RunParams {
            workspace: WorkspaceId(2),
            source: "go",
            repeat: None,
            stream: &streams,
            delay_millis: 0.0,
            speed: None,
            stop_when: None,
            agents: None,
        };
        assert_eq!(
            serde_json::to_value(&p).unwrap(),
            json!({"workspace":2,"source":"go","repeat":null,
                   "stream":{"view":{"everyTicks":1,"maxFps":30.0},
                             "agents":{"everyTicks":2,"maxFps":10.0}}})
        );
        // Protocol 1 fields appear only when set.
        let on_ticks = RunStreams {
            view: Some(StreamRate {
                update: StreamUpdate::Ticks,
                ..StreamRate::new(30.0)
            }),
            agents: None,
            observe: None,
        };
        let p = RunParams {
            workspace: WorkspaceId(2),
            source: "go",
            repeat: None,
            stream: &on_ticks,
            delay_millis: 50.0,
            speed: Some(-10.0),
            stop_when: Some("count turtles = 0"),
            agents: None,
        };
        assert_eq!(
            serde_json::to_value(&p).unwrap(),
            json!({"workspace":2,"source":"go","repeat":null,
                   "stream":{"view":{"everyTicks":1,"maxFps":30.0,"update":"ticks"}},
                   "delayMillis":50.0,"speed":-10.0,"stopWhen":"count turtles = 0"})
        );
        let f: RunFinished = serde_json::from_value(json!({"workspace":2,"run":1,
            "reason":"stopped","iterations":551,"ticks":551.0,"elapsedMillis":9000.5,
            "error":null}))
        .unwrap();
        assert_eq!(f.reason, RunEnd::Stopped);
        assert_eq!(f.iterations, 551);
    }

    #[test]
    fn parses_primitives_and_conversion_results() {
        let r: PrimitivesResult = serde_json::from_value(json!({
            "netlogoVersion": "NetLogo 7.0.4",
            "primitives": [{"name":"of","kind":"reporter",
                "left":{"name":"reporter block","mask":229376},
                "args":[{"name":"agent or agentset","mask":1904}],
                "returns":{"name":"anything","mask":8191},
                "defaultArgCount":1,"minimumArgCount":1,"variadic":false,"precedence":11,
                "isRightAssociative":true,"agentClass":"OTPL","blockAgentClass":"?",
                "introducesContext":true,"extension":null}]}))
        .unwrap();
        let of = &r.primitives[0];
        assert_eq!(of.kind, PrimitiveKind::Reporter);
        assert_eq!(of.left.as_ref().unwrap().mask, 229376);
        assert_eq!(of.args[0].name, "agent or agentset");
        assert_eq!(of.block_agent_class.as_deref(), Some("?"));
        assert_eq!(
            serde_json::to_value(PrimitivesParams {
                extensions: &["nw"]
            })
            .unwrap(),
            json!({"extensions":["nw"]})
        );
        assert_eq!(
            serde_json::to_value(ConvertLegacyParams {
                source_path: "a.nlogo",
                output_path: "a.nlogox"
            })
            .unwrap(),
            json!({"sourcePath":"a.nlogo","outputPath":"a.nlogox"})
        );
        let c: ConvertLegacyResult = serde_json::from_value(json!({"outputPath":"a.nlogox",
            "sourceVersion":"NetLogo 5.0.3","warnings":[]}))
        .unwrap();
        assert_eq!(c.source_version, "NetLogo 5.0.3");
    }

    #[test]
    fn parses_hello_result() {
        let v = json!({"protocolVersion":0,"bridgeVersion":"0.0.0","netlogoVersion":"7.0.4",
            "javaVersion":"17.0.2","javaVendor":"Oracle","extensionsDirs":["a","a/.bundled"],
            "modelsDir":"m"});
        let h: HelloResult = serde_json::from_value(v).unwrap();
        assert_eq!(h.netlogo_version.as_deref(), Some("7.0.4"));
        assert_eq!(h.extensions_dirs.len(), 2);
    }

    #[test]
    fn parses_observe_results() {
        let v = json!({"ticks": null, "values": [
            {"ok": 1.5},
            {"error": {"code": 1001, "message": "Nothing named FOO",
                       "span": {"start": 0, "end": 3, "file": null}}}
        ]});
        let r = ObserveResult::from_json(&v).unwrap();
        assert_eq!(r.ticks, None);
        assert_eq!(r.values[0], Ok(NlValue::Number(1.5)));
        assert_eq!(
            r.values[1].as_ref().unwrap_err().code,
            codes::COMPILER_ERROR
        );
    }

    /// Java writes doubles with Double.toString; these must parse to the same bits.
    #[test]
    fn doubles_from_java_parse_exactly() {
        for (text, expected) in [
            ("0.30000000000000004", 0.1 + 0.2),
            ("4.9E-324", f64::from_bits(1)),
            ("1.7976931348623157E308", f64::MAX),
            ("2.2250738585072014E-308", f64::MIN_POSITIVE),
            ("0.1", 0.1),
            ("123456.78901234567", 123456.789_012_345_67),
        ] {
            let parsed: f64 = serde_json::from_str(text).unwrap();
            assert_eq!(parsed.to_bits(), expected.to_bits(), "{text}");
        }
    }
}
