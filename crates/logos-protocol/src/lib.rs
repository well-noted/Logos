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
pub const PROTOCOL_VERSION: u64 = 0;

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

#[derive(Debug, Clone, Default, Deserialize)]
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObserveStream {
    pub reporters: Vec<String>,
    pub every_ticks: u64,
}

#[derive(Debug, Serialize)]
pub struct RunParams<'a> {
    pub workspace: WorkspaceId,
    pub source: &'a str,
    pub repeat: Option<u64>,
    pub stream: &'a RunStreams,
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
}

/// Result of `workspace.check`: NetLogo's compile errors for the code (at most one; NetLogo
/// stops at the first). Spans are in UTF-16 code units.
#[derive(Debug, Clone, Deserialize)]
pub struct CheckResult {
    pub errors: Vec<ModelError>,
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
#[derive(Debug, Clone, Deserialize)]
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
    fn run_params_and_finished_notification() {
        let streams = RunStreams {
            view: Some(StreamRate {
                every_ticks: 1,
                max_fps: 30.0,
            }),
            agents: Some(StreamRate {
                every_ticks: 2,
                max_fps: 10.0,
            }),
            observe: None,
        };
        let p = RunParams {
            workspace: WorkspaceId(2),
            source: "go",
            repeat: None,
            stream: &streams,
        };
        assert_eq!(
            serde_json::to_value(&p).unwrap(),
            json!({"workspace":2,"source":"go","repeat":null,
                   "stream":{"view":{"everyTicks":1,"maxFps":30.0},
                             "agents":{"everyTicks":2,"maxFps":10.0}}})
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
