//! Messages of the `experiment.*` methods (PROTOCOL.md section 5.4).

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::{DecodeError, NlValue, WorkspaceId};

/// Params of `experiment.load`.
#[derive(Debug, Serialize)]
pub struct ExperimentLoadParams<'a> {
    pub workspace: WorkspaceId,
    pub name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<&'a str>,
}

/// Params of `experiment.run`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentRunParams {
    pub workspace: WorkspaceId,
    pub experiment: u64,
    pub run_number: u64,
    pub update_plots: bool,
}

/// Params of `experiment.command`.
#[derive(Debug, Serialize)]
pub struct ExperimentCommandParams<'a> {
    pub workspace: WorkspaceId,
    pub experiment: u64,
    pub which: &'a str,
}

/// Params of `experiment.close`.
#[derive(Debug, Serialize)]
pub struct ExperimentCloseParams {
    pub experiment: u64,
}

/// A runtime error or failed metric reported by BehaviorSpace.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct LabError {
    /// The Java exception's class name, for example `RuntimePrimitiveException`.
    #[serde(rename = "type")]
    pub kind: String,
    pub message: String,
}

/// A setting or measured value.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    /// The value, or the error a metric failed with.
    pub value: Result<NlValue, LabError>,
    /// What BehaviorSpace writes for the value in table and spreadsheet output.
    pub text: String,
}

impl Cell {
    fn from_json(v: &Json) -> Result<Cell, DecodeError> {
        let text = str_field(v, "text")?;
        let value = if let Some(e) = v.get("error") {
            Err(serde_json::from_value(e.clone())?)
        } else {
            match v.get("value") {
                // A variable a sub-experiment leaves unset has no value.
                None | Some(Json::Null) => Ok(NlValue::Other(text.clone())),
                Some(j) => Ok(NlValue::from_json(j)
                    .map_err(|m| DecodeError::Shape(format!("bad value: {m}")))?),
            }
        };
        Ok(Cell { value, text })
    }
}

/// One variable's setting in a run.
#[derive(Debug, Clone, PartialEq)]
pub struct Setting {
    pub name: String,
    pub cell: Cell,
}

fn settings(v: &Json) -> Result<Vec<Setting>, DecodeError> {
    array(Some(v), "settings")?
        .iter()
        .map(|s| {
            Ok(Setting {
                name: str_field(s, "name")?,
                cell: Cell::from_json(s)?,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldDimensions {
    pub min_pxcor: i64,
    pub max_pxcor: i64,
    pub min_pycor: i64,
    pub max_pycor: i64,
}

/// Result of `experiment.load`: the experiment as NetLogo read it, and its runs.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedExperiment {
    /// The bridge's id for the loaded experiment.
    pub id: u64,
    pub name: String,
    pub repetitions: u64,
    pub sequential_run_order: bool,
    pub run_metrics_every_step: bool,
    pub run_metrics_condition: String,
    pub time_limit: u64,
    pub exit_condition: String,
    pub pre_experiment: String,
    pub setup: String,
    pub go: String,
    pub post_run: String,
    pub post_experiment: String,
    /// Metrics as BehaviorSpace uses them (commented-out metrics removed).
    pub metrics: Vec<String>,
    /// The variable columns of table output, in order.
    pub variables: Vec<String>,
    /// The settings of run 1, 2, ... in BehaviorSpace order.
    pub runs: Vec<Vec<Setting>>,
    pub world_dimensions: WorldDimensions,
}

impl LoadedExperiment {
    pub fn from_json(v: &Json) -> Result<LoadedExperiment, DecodeError> {
        Ok(LoadedExperiment {
            id: u64_field(v, "experiment")?,
            name: str_field(v, "name")?,
            repetitions: u64_field(v, "repetitions")?,
            sequential_run_order: bool_field(v, "sequentialRunOrder")?,
            run_metrics_every_step: bool_field(v, "runMetricsEveryStep")?,
            run_metrics_condition: str_field(v, "runMetricsCondition")?,
            time_limit: u64_field(v, "timeLimit")?,
            exit_condition: str_field(v, "exitCondition")?,
            pre_experiment: str_field(v, "preExperiment")?,
            setup: str_field(v, "setup")?,
            go: str_field(v, "go")?,
            post_run: str_field(v, "postRun")?,
            post_experiment: str_field(v, "postExperiment")?,
            metrics: strings(v, "metrics")?,
            variables: strings(v, "variables")?,
            runs: array(v.get("runs"), "runs")?
                .iter()
                .map(settings)
                .collect::<Result<_, _>>()?,
            world_dimensions: serde_json::from_value(
                v.get("worldDimensions")
                    .cloned()
                    .ok_or_else(|| missing("worldDimensions"))?,
            )?,
        })
    }

    /// True when metrics are measured during the run, not only at its end.
    pub fn measures_during_run(&self) -> bool {
        self.run_metrics_every_step || !self.run_metrics_condition.trim().is_empty()
    }
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LabRunEnd {
    /// Ended normally; the post-run commands ran.
    Completed,
    /// Stopped by the client after a step; not measured at the end.
    Halted,
    /// Abandoned by BehaviorSpace; the errors say why.
    Failed,
}

/// One row of measurements.
#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    pub step: u64,
    /// One cell per metric.
    pub values: Vec<Cell>,
}

/// Result of `experiment.run`.
#[derive(Debug, Clone, PartialEq)]
pub struct LabRunResult {
    pub run_number: u64,
    pub settings: Vec<Setting>,
    pub reason: LabRunEnd,
    pub steps: u64,
    pub measurements: Vec<Measurement>,
    pub errors: Vec<LabError>,
    pub elapsed_millis: f64,
}

impl LabRunResult {
    pub fn from_json(v: &Json) -> Result<LabRunResult, DecodeError> {
        Ok(LabRunResult {
            run_number: u64_field(v, "runNumber")?,
            settings: settings(v.get("settings").ok_or_else(|| missing("settings"))?)?,
            reason: serde_json::from_value(
                v.get("reason").cloned().ok_or_else(|| missing("reason"))?,
            )?,
            steps: u64_field(v, "steps")?,
            measurements: array(v.get("measurements"), "measurements")?
                .iter()
                .map(|m| {
                    Ok(Measurement {
                        step: u64_field(m, "step")?,
                        values: array(m.get("values"), "values")?
                            .iter()
                            .map(Cell::from_json)
                            .collect::<Result<_, _>>()?,
                    })
                })
                .collect::<Result<_, DecodeError>>()?,
            errors: serde_json::from_value(
                v.get("errors").cloned().ok_or_else(|| missing("errors"))?,
            )?,
            elapsed_millis: v.get("elapsedMillis").and_then(Json::as_f64).unwrap_or(0.0),
        })
    }
}

/// Result of `experiment.command`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ExperimentCommandResult {
    pub error: Option<LabError>,
}

fn missing(name: &str) -> DecodeError {
    DecodeError::Shape(format!("missing {name}"))
}

fn str_field(v: &Json, name: &str) -> Result<String, DecodeError> {
    v.get(name)
        .and_then(Json::as_str)
        .map(str::to_owned)
        .ok_or_else(|| missing(name))
}

fn u64_field(v: &Json, name: &str) -> Result<u64, DecodeError> {
    v.get(name)
        .and_then(Json::as_u64)
        .ok_or_else(|| missing(name))
}

fn bool_field(v: &Json, name: &str) -> Result<bool, DecodeError> {
    v.get(name)
        .and_then(Json::as_bool)
        .ok_or_else(|| missing(name))
}

fn array<'a>(v: Option<&'a Json>, name: &str) -> Result<&'a Vec<Json>, DecodeError> {
    v.and_then(Json::as_array).ok_or_else(|| missing(name))
}

fn strings(v: &Json, name: &str) -> Result<Vec<String>, DecodeError> {
    array(v.get(name), name)?
        .iter()
        .map(|s| s.as_str().map(str::to_owned).ok_or_else(|| missing(name)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decodes_a_run_result() {
        let v = json!({
            "runNumber": 3,
            "settings": [{"name": "mode", "value": "zones", "text": "zones"},
                         {"name": "b", "value": null, "text": "null"}],
            "reason": "completed",
            "steps": 2,
            "measurements": [
                {"step": 0, "values": [{"value": 1.5, "text": "1.5"},
                    {"error": {"type": "RuntimePrimitiveException", "message": "boom"},
                     "text": "<RuntimePrimitiveException>"}]}
            ],
            "errors": [{"type": "RuntimePrimitiveException", "message": "boom"}],
            "elapsedMillis": 1.25
        });
        let r = LabRunResult::from_json(&v).unwrap();
        assert_eq!(r.run_number, 3);
        assert_eq!(r.reason, LabRunEnd::Completed);
        assert_eq!(
            r.settings[0].cell.value,
            Ok(NlValue::String("zones".into()))
        );
        assert_eq!(r.settings[1].cell.value, Ok(NlValue::Other("null".into())));
        assert_eq!(r.measurements[0].values[0].value, Ok(NlValue::Number(1.5)));
        assert_eq!(
            r.measurements[0].values[1].value.as_ref().unwrap_err().kind,
            "RuntimePrimitiveException"
        );
        assert_eq!(r.errors.len(), 1);
    }

    #[test]
    fn serializes_params() {
        let p = ExperimentRunParams {
            workspace: WorkspaceId(2),
            experiment: 1,
            run_number: 5,
            update_plots: false,
        };
        assert_eq!(
            serde_json::to_value(&p).unwrap(),
            json!({"workspace": 2, "experiment": 1, "runNumber": 5, "updatePlots": false})
        );
        let l = ExperimentLoadParams {
            workspace: WorkspaceId(2),
            name: "E1",
            file: None,
        };
        assert_eq!(
            serde_json::to_value(&l).unwrap(),
            json!({"workspace": 2, "name": "E1"})
        );
    }
}
