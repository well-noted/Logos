//! The Logos bridge protocol, Rust side. The specification is `protocol/PROTOCOL.md`; this
//! crate and `bridge/` must change together with it.

pub mod binary;
pub mod frame;
pub mod value;

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

pub use binary::{DataFrame, DataFrameError, ViewFrame};
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
                   "stream":{"view":{"everyTicks":1,"maxFps":30.0}}})
        );
        let f: RunFinished = serde_json::from_value(json!({"workspace":2,"run":1,
            "reason":"stopped","iterations":551,"ticks":551.0,"elapsedMillis":9000.5,
            "error":null}))
        .unwrap();
        assert_eq!(f.reason, RunEnd::Stopped);
        assert_eq!(f.iterations, 551);
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
