//! NetLogo values as they appear in protocol messages (PROTOCOL.md section 7).

use serde_json::Value as Json;

#[derive(Debug, Clone, PartialEq)]
pub enum NlValue {
    Number(f64),
    String(String),
    Boolean(bool),
    List(Vec<NlValue>),
    Nobody,
    Turtle(i64),
    Patch(i64, i64),
    Link {
        ends: (i64, i64),
        breed: String,
    },
    AgentSet {
        kind: AgentKind,
        count: u64,
    },
    Procedure(String),
    Extension {
        extension: String,
        display: String,
    },
    /// A value the protocol has no specific shape for, shown as NetLogo prints it.
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Turtles,
    Patches,
    Links,
    Observer,
}

impl NlValue {
    pub fn from_json(v: &Json) -> Result<NlValue, String> {
        Ok(match v {
            Json::Number(n) => NlValue::Number(
                n.as_f64()
                    .ok_or_else(|| format!("number {n} is not a double"))?,
            ),
            Json::String(s) => NlValue::String(s.clone()),
            Json::Bool(b) => NlValue::Boolean(*b),
            Json::Array(items) => NlValue::List(
                items
                    .iter()
                    .map(NlValue::from_json)
                    .collect::<Result<_, _>>()?,
            ),
            Json::Object(o) => {
                if o.contains_key("nobody") {
                    NlValue::Nobody
                } else if let Some(who) = o.get("turtle") {
                    NlValue::Turtle(int(who)?)
                } else if let Some(Json::Array(xy)) = o.get("patch") {
                    match xy.as_slice() {
                        [x, y] => NlValue::Patch(int(x)?, int(y)?),
                        _ => return Err("patch needs two coordinates".into()),
                    }
                } else if let Some(Json::Array(ends)) = o.get("link") {
                    let breed = str_field(o, "breed")?;
                    match ends.as_slice() {
                        [a, b] => NlValue::Link {
                            ends: (int(a)?, int(b)?),
                            breed,
                        },
                        _ => return Err("link needs two ends".into()),
                    }
                } else if let Some(kind) = o.get("agentset") {
                    let kind = match kind.as_str() {
                        Some("turtles") => AgentKind::Turtles,
                        Some("patches") => AgentKind::Patches,
                        Some("links") => AgentKind::Links,
                        Some("observer") => AgentKind::Observer,
                        _ => return Err(format!("unknown agentset kind {kind}")),
                    };
                    let count = o
                        .get("count")
                        .and_then(Json::as_u64)
                        .ok_or("agentset needs a count")?;
                    NlValue::AgentSet { kind, count }
                } else if o.contains_key("procedure") {
                    NlValue::Procedure(str_field(o, "procedure")?)
                } else if o.contains_key("extension") {
                    NlValue::Extension {
                        extension: str_field(o, "extension")?,
                        display: str_field(o, "display")?,
                    }
                } else if o.contains_key("display") {
                    NlValue::Other(str_field(o, "display")?)
                } else {
                    return Err(format!("unknown value object {v}"));
                }
            }
            Json::Null => return Err("null is not a NetLogo value".into()),
        })
    }
}

fn int(v: &Json) -> Result<i64, String> {
    v.as_i64().ok_or_else(|| format!("{v} is not an integer"))
}

fn str_field(o: &serde_json::Map<String, Json>, name: &str) -> Result<String, String> {
    o.get(name)
        .and_then(Json::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("missing string field {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The shapes asserted by the bridge's WorkspaceTest.valuesUseTheProtocolEncoding.
    #[test]
    fn decodes_every_protocol_shape() {
        let cases = [
            (
                json!([1.0, "a", true, [2.0]]),
                NlValue::List(vec![
                    NlValue::Number(1.0),
                    NlValue::String("a".into()),
                    NlValue::Boolean(true),
                    NlValue::List(vec![NlValue::Number(2.0)]),
                ]),
            ),
            (json!({"nobody": true}), NlValue::Nobody),
            (
                json!({"agentset": "turtles", "count": 2}),
                NlValue::AgentSet {
                    kind: AgentKind::Turtles,
                    count: 2,
                },
            ),
            (json!({"patch": [0, 1]}), NlValue::Patch(0, 1)),
            (json!({"turtle": 1}), NlValue::Turtle(1)),
            (
                json!({"link": [0, 1], "breed": "links"}),
                NlValue::Link {
                    ends: (0, 1),
                    breed: "links".into(),
                },
            ),
            (
                json!({"procedure": "(anonymous reporter: [ x -> x ])"}),
                NlValue::Procedure("(anonymous reporter: [ x -> x ])".into()),
            ),
            (
                json!({"extension": "table", "display": "{{table: }}"}),
                NlValue::Extension {
                    extension: "table".into(),
                    display: "{{table: }}".into(),
                },
            ),
            (json!({"display": "?"}), NlValue::Other("?".into())),
        ];
        for (json, expected) in cases {
            assert_eq!(NlValue::from_json(&json).unwrap(), expected, "{json}");
        }
    }

    #[test]
    fn rejects_malformed_values() {
        for bad in [
            json!(null),
            json!({}),
            json!({"patch": [1]}),
            json!({"agentset": "cats", "count": 1}),
            json!({"turtle": 1.5}),
        ] {
            assert!(NlValue::from_json(&bad).is_err(), "{bad}");
        }
    }
}
