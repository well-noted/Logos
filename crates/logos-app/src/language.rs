//! The names NetLogo knows, by class, for highlighting code in the Code tab and the Command
//! Center: command and reporter primitives, keywords, constants, built-in agent variables and
//! the breed primitive patterns. Read once from the bridge's primitive table (D-01).

use logos_bridge_client::Bridge;
use logos_protocol::{BreedKind, PrimitiveKind, PrimitivesResult};
use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageNames {
    pub commands: Vec<String>,
    pub reporters: Vec<String>,
    pub keywords: Vec<String>,
    pub constants: Vec<String>,
    pub variables: Vec<String>,
    /// `create-<breeds>` and the like: (pattern, "command" | "reporter", breed kind).
    pub breed_patterns: Vec<(String, String, String)>,
}

impl From<&PrimitivesResult> for LanguageNames {
    fn from(p: &PrimitivesResult) -> Self {
        let kind = |k: PrimitiveKind| match k {
            PrimitiveKind::Command => "command",
            PrimitiveKind::Reporter => "reporter",
        };
        let mut n = LanguageNames::default();
        for prim in &p.primitives {
            // Internal primitives (`__foo`) are left uncolored.
            if prim.name.starts_with("__") {
                continue;
            }
            match prim.kind {
                PrimitiveKind::Command => n.commands.push(prim.name.clone()),
                PrimitiveKind::Reporter => n.reporters.push(prim.name.clone()),
            }
        }
        n.keywords = p.keywords.clone();
        n.constants = p.constants.iter().map(|c| c.name.to_lowercase()).collect();
        let v = &p.agent_variables;
        for list in [&v.observer, &v.turtle, &v.patch, &v.link] {
            for var in list {
                let name = var.name.to_lowercase();
                if !n.variables.contains(&name) {
                    n.variables.push(name);
                }
            }
        }
        n.breed_patterns = p
            .breed_primitives
            .iter()
            .map(|b| {
                let k = match b.breed_kind {
                    BreedKind::Turtle => "turtle",
                    BreedKind::DirectedLink => "directed-link",
                    BreedKind::UndirectedLink => "undirected-link",
                };
                (
                    b.primitive.name.clone(),
                    kind(b.primitive.kind).to_string(),
                    k.to_string(),
                )
            })
            .collect();
        n
    }
}

/// Reads the core primitive table from a bridge.
pub fn load(bridge: &Bridge) -> Result<LanguageNames, String> {
    bridge
        .primitives(&[])
        .map(|p| LanguageNames::from(&p))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_by_class() {
        let p: PrimitivesResult = serde_json::from_value(serde_json::json!({
            "netlogoVersion": "NetLogo 7.0.4",
            "primitives": [
                {"name": "show", "kind": "command", "left": null, "args": [], "returns": null,
                 "defaultArgCount": 1, "minimumArgCount": 1, "variadic": false, "precedence": 0,
                 "isRightAssociative": false, "agentClass": "OTPL", "blockAgentClass": null,
                 "introducesContext": false, "canBeConcise": false, "extension": null},
                {"name": "count", "kind": "reporter", "left": null, "args": [], "returns": null,
                 "defaultArgCount": 1, "minimumArgCount": 1, "variadic": false, "precedence": 0,
                 "isRightAssociative": false, "agentClass": "OTPL", "blockAgentClass": null,
                 "introducesContext": false, "canBeConcise": false, "extension": null},
                {"name": "__internal", "kind": "command", "left": null, "args": [], "returns": null,
                 "defaultArgCount": 0, "minimumArgCount": 0, "variadic": false, "precedence": 0,
                 "isRightAssociative": false, "agentClass": "OTPL", "blockAgentClass": null,
                 "introducesContext": false, "canBeConcise": false, "extension": null}
            ],
            "constants": [{"name": "TRUE", "value": true}],
            "keywords": ["to", "end"],
            "agentVariables": {"observer": [], "turtle": [{"name": "XCOR", "type": {"name": "number", "mask": 1}}], "patch": [], "link": []},
            "breedPrimitives": []
        }))
        .unwrap();
        let n = LanguageNames::from(&p);
        assert_eq!(n.commands, ["show"]);
        assert_eq!(n.reporters, ["count"]);
        assert_eq!(n.constants, ["true"]);
        assert_eq!(n.variables, ["xcor"]);
    }
}
