//! BehaviorSpace experiments as stored in `.nlogox` files.
//!
//! Reads `enumeratedValueSet` and `steppedValueSet`, run order, repetitions, a time limit,
//! the stop condition, metrics and setup/go commands, enough to list an experiment's runs
//! without starting NetLogo. Anything else is reported as unsupported rather than ignored.
//! Running an experiment doesn't use this parser: the bridge reads experiments with NetLogo's
//! own loader (PROTOCOL.md section 5.4).

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ExperimentError {
    #[error("XML error: {0}")]
    Xml(String),
    #[error("experiment `{experiment}` uses {feature}, which is not supported yet")]
    Unsupported { experiment: String, feature: String },
    #[error("experiment `{0}` not found")]
    NotFound(String),
    #[error("bad value in experiment `{experiment}`: {detail}")]
    BadValue { experiment: String, detail: String },
}

/// One BehaviorSpace experiment.
#[derive(Debug, Clone, PartialEq)]
pub struct Experiment {
    pub name: String,
    pub repetitions: u64,
    pub sequential_run_order: bool,
    pub run_metrics_every_step: bool,
    pub setup: String,
    pub go: String,
    /// Maximum number of steps; 0 means no limit.
    pub time_limit: u64,
    pub exit_condition: String,
    pub metrics: Vec<String>,
    /// Variables in the order they appear in the file.
    pub variables: Vec<Variable>,
}

/// A variable and the NetLogo literals it takes, in order.
#[derive(Debug, Clone, PartialEq)]
pub struct Variable {
    pub name: String,
    pub values: Vec<String>,
}

/// The variable settings for one run.
#[derive(Debug, Clone, PartialEq)]
pub struct RunSettings {
    /// BehaviorSpace's run number, starting at 1.
    pub run_number: u64,
    /// (variable, NetLogo literal) pairs in the experiment's variable order.
    pub values: Vec<(String, String)>,
}

impl RunSettings {
    /// The commands that set this run's variables, one `set` per variable.
    pub fn set_commands(&self) -> String {
        self.values
            .iter()
            .map(|(name, literal)| format!("set {name} {literal}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Experiment {
    /// The runs of this experiment in BehaviorSpace order (`LabProtocol.refElements` at
    /// v7.0.4).
    ///
    /// With sequential run order, the last variable changes fastest and each combination is
    /// repeated `repetitions` times before moving to the next one. Otherwise the first variable
    /// changes fastest and the whole set of combinations is repeated.
    pub fn runs(&self) -> Vec<RunSettings> {
        let count: usize = self.variables.iter().map(|v| v.values.len()).product();
        let combo = |mut i: usize| -> Vec<(String, String)> {
            let mut digits = vec![0; self.variables.len()];
            let order: Vec<usize> = if self.sequential_run_order {
                (0..self.variables.len()).rev().collect()
            } else {
                (0..self.variables.len()).collect()
            };
            for k in order {
                let n = self.variables[k].values.len();
                digits[k] = i % n;
                i /= n;
            }
            self.variables
                .iter()
                .zip(digits)
                .map(|(v, d)| (v.name.clone(), v.values[d].clone()))
                .collect()
        };
        let mut ordered = Vec::new();
        if self.sequential_run_order {
            for i in 0..count {
                let c = combo(i);
                for _ in 0..self.repetitions {
                    ordered.push(c.clone());
                }
            }
        } else {
            for _ in 0..self.repetitions {
                ordered.extend((0..count).map(combo));
            }
        }
        ordered
            .into_iter()
            .enumerate()
            .map(|(i, values)| RunSettings {
                run_number: i as u64 + 1,
                values,
            })
            .collect()
    }
}

/// Reads every experiment in a `.nlogox` document.
pub fn parse_experiments(xml: &str) -> Result<Vec<Experiment>, ExperimentError> {
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    let mut current: Option<Experiment> = None;
    let mut text_target: Option<&'static str> = None;
    let mut text = String::new();
    let mut variable: Option<Variable> = None;
    let mut stepped: Option<(String, f64, f64, f64)> = None;

    loop {
        let event = reader
            .read_event()
            .map_err(|e| ExperimentError::Xml(e.to_string()))?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) if current.is_none() => {
                if e.name().as_ref() == b"experiment" {
                    current = Some(Experiment {
                        name: attr(e, "name")?.unwrap_or_default(),
                        repetitions: attr(e, "repetitions")?
                            .and_then(|r| r.parse().ok())
                            .unwrap_or(1),
                        sequential_run_order: attr(e, "sequentialRunOrder")?.as_deref()
                            != Some("false"),
                        run_metrics_every_step: attr(e, "runMetricsEveryStep")?.as_deref()
                            == Some("true"),
                        setup: String::new(),
                        go: String::new(),
                        time_limit: 0,
                        exit_condition: String::new(),
                        metrics: Vec::new(),
                        variables: Vec::new(),
                    });
                }
            }
            Event::Start(ref e) | Event::Empty(ref e) => {
                let empty = matches!(event, Event::Empty(_));
                let exp = current.as_mut().expect("inside an experiment");
                let unsupported = |feature: &str| ExperimentError::Unsupported {
                    experiment: exp.name.clone(),
                    feature: feature.to_owned(),
                };
                match e.name().as_ref() {
                    // An empty element like <setup/> has no text and no end tag.
                    b"setup" | b"go" | b"metric" | b"exitCondition" | b"preExperiment"
                    | b"postRun" | b"postExperiment"
                        if empty => {}
                    b"setup" => text_target = Some("setup"),
                    b"go" => text_target = Some("go"),
                    b"metric" => text_target = Some("metric"),
                    b"exitCondition" => text_target = Some("exitCondition"),
                    b"timeLimit" => {
                        exp.time_limit = attr(e, "steps")?.and_then(|s| s.parse().ok()).unwrap_or(0)
                    }
                    b"enumeratedValueSet" => {
                        variable = Some(Variable {
                            name: attr(e, "variable")?.unwrap_or_default(),
                            values: Vec::new(),
                        })
                    }
                    b"value" => {
                        if let (Some(v), Some(value)) = (variable.as_mut(), attr(e, "value")?) {
                            v.values.push(value);
                        }
                    }
                    b"steppedValueSet" => {
                        let num = |name: &str| -> Result<f64, ExperimentError> {
                            attr(e, name)?.and_then(|s| s.parse().ok()).ok_or_else(|| {
                                ExperimentError::BadValue {
                                    experiment: exp.name.clone(),
                                    detail: format!("steppedValueSet needs a numeric {name}"),
                                }
                            })
                        };
                        stepped = Some((
                            attr(e, "variable")?.unwrap_or_default(),
                            num("first")?,
                            num("step")?,
                            num("last")?,
                        ));
                    }
                    b"preExperiment" | b"postRun" | b"postExperiment" => {
                        text_target = Some("ignored-if-empty")
                    }
                    b"runMetricsCondition" => return Err(unsupported("a run metrics condition")),
                    b"subExperiment" => return Err(unsupported("sub-experiments")),
                    _ => {}
                }
            }
            Event::Text(t) => {
                if text_target.is_some() {
                    text.push_str(
                        &t.decode()
                            .map_err(|e| ExperimentError::Xml(e.to_string()))?,
                    );
                }
            }
            Event::CData(t) => {
                if text_target.is_some() {
                    text.push_str(&String::from_utf8_lossy(&t));
                }
            }
            Event::GeneralRef(r) => {
                if text_target.is_some() {
                    let name = String::from_utf8_lossy(&r).into_owned();
                    text.push_str(match name.as_str() {
                        "lt" => "<",
                        "gt" => ">",
                        "amp" => "&",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => return Err(ExperimentError::Xml(format!("unknown entity &{name};"))),
                    });
                }
            }
            Event::End(e) => {
                let Some(exp) = current.as_mut() else {
                    continue;
                };
                match e.name().as_ref() {
                    b"setup" | b"go" | b"metric" | b"exitCondition" | b"preExperiment"
                    | b"postRun" | b"postExperiment" => {
                        let value = std::mem::take(&mut text);
                        match text_target.take() {
                            Some("setup") => exp.setup = value,
                            Some("go") => exp.go = value,
                            Some("metric") => exp.metrics.push(value),
                            Some("exitCondition") => exp.exit_condition = value,
                            _ => {
                                if !value.trim().is_empty() {
                                    return Err(ExperimentError::Unsupported {
                                        experiment: exp.name.clone(),
                                        feature: format!(
                                            "<{}> commands",
                                            String::from_utf8_lossy(e.name().as_ref())
                                        ),
                                    });
                                }
                            }
                        }
                    }
                    b"enumeratedValueSet" => {
                        if let Some(v) = variable.take() {
                            exp.variables.push(v);
                        }
                    }
                    b"steppedValueSet" => {
                        if let Some((name, first, step, last)) = stepped.take() {
                            exp.variables
                                .push(stepped_values(&exp.name, name, first, step, last)?);
                        }
                    }
                    b"experiment" => out.push(current.take().expect("inside an experiment")),
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

/// Expands a stepped value set the way BehaviorSpace does: first, first + step, ... while the
/// value does not pass `last`. Values are computed by multiplication to avoid accumulating
/// rounding error, and written as NetLogo number literals.
fn stepped_values(
    experiment: &str,
    name: String,
    first: f64,
    step: f64,
    last: f64,
) -> Result<Variable, ExperimentError> {
    if step == 0.0 || (last - first) / step < 0.0 {
        return Err(ExperimentError::BadValue {
            experiment: experiment.to_owned(),
            detail: format!("stepped values for {name} never reach {last}"),
        });
    }
    let count = ((last - first) / step + 1e-9).floor() as u64 + 1;
    let values = (0..count)
        .map(|i| format_number(first + step * i as f64))
        .collect();
    Ok(Variable { name, values })
}

fn format_number(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}

fn attr(e: &BytesStart, name: &str) -> Result<Option<String>, ExperimentError> {
    for a in e.attributes() {
        let a = a.map_err(|e| ExperimentError::Xml(e.to_string()))?;
        if a.key.as_ref() == name.as_bytes() {
            let v = a
                .unescape_value()
                .map_err(|e| ExperimentError::Xml(e.to_string()))?;
            return Ok(Some(v.into_owned()));
        }
    }
    Ok(None)
}

/// Finds one experiment by name.
pub fn find_experiment(xml: &str, name: &str) -> Result<Experiment, ExperimentError> {
    parse_experiments(xml)?
        .into_iter()
        .find(|e| e.name == name)
        .ok_or_else(|| ExperimentError::NotFound(name.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<model version="NetLogo 7.0.4">
  <code><![CDATA[to setup end to go end]]></code>
  <experiments>
    <experiment name="E1" repetitions="2" sequentialRunOrder="true" runMetricsEveryStep="false">
      <setup>setup</setup>
      <go>go</go>
      <timeLimit steps="100"></timeLimit>
      <metrics>
        <metric>count turtles</metric>
        <metric>mean [x] of patches with [ x &lt; 3 ]</metric>
      </metrics>
      <constants>
        <enumeratedValueSet variable="a">
          <value value="1"></value>
          <value value="2"></value>
        </enumeratedValueSet>
        <enumeratedValueSet variable="mode">
          <value value="&quot;zones&quot;"></value>
        </enumeratedValueSet>
        <steppedValueSet variable="s" first="0" step="0.1" last="0.3"></steppedValueSet>
      </constants>
    </experiment>
    <experiment name="E2" repetitions="1" sequentialRunOrder="false" runMetricsEveryStep="false">
      <setup><![CDATA[setup
set x 1]]></setup>
      <go>go</go>
    </experiment>
  </experiments>
</model>"#;

    #[test]
    fn parses_experiments() {
        let all = parse_experiments(XML).unwrap();
        assert_eq!(all.len(), 2);
        let e = &all[0];
        assert_eq!(e.name, "E1");
        assert_eq!(e.repetitions, 2);
        assert_eq!(e.time_limit, 100);
        assert_eq!(e.metrics[1], "mean [x] of patches with [ x < 3 ]");
        assert_eq!(e.variables[1].values, vec!["\"zones\""]);
        assert_eq!(
            e.variables[2].values,
            vec!["0", "0.1", "0.2", "0.30000000000000004"]
        );
        assert_eq!(all[1].setup, "setup\nset x 1");
        assert!(!all[1].sequential_run_order);
    }

    #[test]
    fn runs_follow_behaviorspace_order() {
        let e = find_experiment(XML, "E1").unwrap();
        let runs = e.runs();
        // 2 values of a x 1 of mode x 4 of s x 2 repetitions.
        assert_eq!(runs.len(), 16);
        assert_eq!(runs[0].run_number, 1);
        // Sequential order: each combination twice in a row, last variable fastest.
        assert_eq!(runs[0].values, runs[1].values);
        assert_eq!(runs[2].values[2], ("s".to_owned(), "0.1".to_owned()));
        assert_eq!(runs[8].values[0], ("a".to_owned(), "2".to_owned()));
        assert_eq!(
            runs[0].set_commands(),
            "set a 1\nset mode \"zones\"\nset s 0"
        );
    }

    #[test]
    fn interleaved_runs_change_the_first_variable_fastest() {
        let mut e = find_experiment(XML, "E1").unwrap();
        e.sequential_run_order = false;
        let runs = e.runs();
        assert_eq!(runs.len(), 16);
        let a_s = |r: &RunSettings| (r.values[0].1.clone(), r.values[2].1.clone());
        assert_eq!(a_s(&runs[0]), ("1".into(), "0".into()));
        assert_eq!(a_s(&runs[1]), ("2".into(), "0".into()));
        assert_eq!(a_s(&runs[2]), ("1".into(), "0.1".into()));
        // The second repetition starts over.
        assert_eq!(runs[8].values, runs[0].values);
    }

    #[test]
    fn rejects_unsupported_features() {
        let xml = XML.replace(
            "<go>go</go>\n      <timeLimit",
            "<go>go</go>\n      <runMetricsCondition>true</runMetricsCondition>\n      <timeLimit",
        );
        assert!(matches!(
            parse_experiments(&xml),
            Err(ExperimentError::Unsupported { .. })
        ));
        assert_eq!(
            find_experiment(XML, "nope"),
            Err(ExperimentError::NotFound("nope".into()))
        );
    }
}
