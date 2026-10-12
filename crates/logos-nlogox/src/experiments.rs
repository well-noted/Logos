//! Editing BehaviorSpace experiments stored in a model (spec part E, E4) with span edits:
//! attributes, commands, metrics and value sets of an `<experiment>`, and duplicating or
//! deleting one. Elements are only changed or added where their place is known from the file
//! itself: a missing `<timeLimit>` or `<exitCondition>` is not added, because the order
//! NetLogo writes them in isn't settled here yet.

use crate::text::{encode_attribute, encode_content};
use crate::{EditError, Element, Nlogox};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ExperimentEditError {
    #[error("no experiment {0}")]
    NoSuchExperiment(usize),
    #[error("experiment {index} has no <{part}> element to change")]
    MissingPart { index: usize, part: String },
    #[error("experiment {index} has no variable {variable}")]
    NoSuchVariable { index: usize, variable: String },
    #[error("an experiment named {0} already exists")]
    NameInUse(String),
    #[error(transparent)]
    Edit(#[from] EditError),
}

/// A value set of an experiment as written in the file.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(
    feature = "serde",
    serde(
        tag = "kind",
        rename_all = "camelCase",
        rename_all_fields = "camelCase"
    )
)]
pub enum ValueSet {
    Enumerated {
        variable: String,
        /// NetLogo literals, as in the `value` attributes.
        values: Vec<String>,
    },
    Stepped {
        variable: String,
        first: String,
        step: String,
        last: String,
    },
}

impl ValueSet {
    pub fn variable(&self) -> &str {
        match self {
            ValueSet::Enumerated { variable, .. } | ValueSet::Stepped { variable, .. } => variable,
        }
    }
}

fn find_path<'a>(e: &'a Element, path: &mut Vec<usize>, pred: &dyn Fn(&Element) -> bool) -> bool {
    for (i, c) in e.elements().enumerate() {
        path.push(i);
        if pred(c) || find_path(c, path, pred) {
            return true;
        }
        path.pop();
    }
    false
}

impl Nlogox {
    /// The `<experiment>` elements of the `<experiments>` section, in file order.
    pub fn experiment_elements(&self) -> Vec<&Element> {
        self.section("experiments")
            .map(|s| s.elements().filter(|e| e.name == "experiment").collect())
            .unwrap_or_default()
    }

    fn experiment_path(&self, index: usize) -> Result<Vec<usize>, ExperimentEditError> {
        let section = self
            .section_path("experiments")
            .ok_or(ExperimentEditError::NoSuchExperiment(index))?;
        let s = self.root().at(&section).expect("section path");
        let i = s
            .elements()
            .enumerate()
            .filter(|(_, e)| e.name == "experiment")
            .nth(index)
            .map(|(i, _)| i)
            .ok_or(ExperimentEditError::NoSuchExperiment(index))?;
        Ok([section, vec![i]].concat())
    }

    /// The index of the experiment with this name.
    pub fn experiment_index(&self, name: &str) -> Option<usize> {
        self.experiment_elements()
            .iter()
            .position(|e| self.attribute_value(e, "name").as_deref() == Some(name))
    }

    /// The names of the elements inside an experiment, in order.
    pub fn experiment_parts(&self, index: usize) -> Vec<String> {
        self.experiment_elements()
            .get(index)
            .map(|e| e.elements().map(|c| c.name.clone()).collect())
            .unwrap_or_default()
    }

    /// The value sets of an experiment as written.
    pub fn experiment_value_sets(&self, index: usize) -> Vec<ValueSet> {
        let Some(e) = self.experiment_elements().get(index).copied() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        self.collect_value_sets(e, &mut out);
        out
    }

    fn collect_value_sets(&self, e: &Element, out: &mut Vec<ValueSet>) {
        for c in e.elements() {
            let attr = |n: &str| self.attribute_value(c, n).unwrap_or_default();
            match c.name.as_str() {
                "enumeratedValueSet" => out.push(ValueSet::Enumerated {
                    variable: attr("variable"),
                    values: c
                        .elements()
                        .filter(|v| v.name == "value")
                        .map(|v| self.attribute_value(v, "value").unwrap_or_default())
                        .collect(),
                }),
                "steppedValueSet" => out.push(ValueSet::Stepped {
                    variable: attr("variable"),
                    first: attr("first"),
                    step: attr("step"),
                    last: attr("last"),
                }),
                _ => self.collect_value_sets(c, out),
            }
        }
    }

    /// The path of the first element inside the experiment for which `pred` holds.
    fn find_in_experiment(
        &self,
        index: usize,
        pred: &dyn Fn(&Element) -> bool,
    ) -> Result<Option<Vec<usize>>, ExperimentEditError> {
        let mut path = self.experiment_path(index)?;
        let e = self.root().at(&path).expect("experiment path");
        Ok(find_path(e, &mut path, pred).then_some(path))
    }

    fn part_path(&self, index: usize, part: &str) -> Result<Vec<usize>, ExperimentEditError> {
        let path = self.experiment_path(index)?;
        let e = self.root().at(&path).expect("experiment path");
        let i = e.elements().position(|c| c.name == part).ok_or_else(|| {
            ExperimentEditError::MissingPart {
                index,
                part: part.to_string(),
            }
        })?;
        Ok([path, vec![i]].concat())
    }

    /// Sets an attribute of the `<experiment>` element: `name`, `repetitions`,
    /// `sequentialRunOrder`, `runMetricsEveryStep`.
    pub fn set_experiment_attribute(
        &mut self,
        index: usize,
        name: &str,
        value: &str,
    ) -> Result<(), ExperimentEditError> {
        if name == "name"
            && let Some(other) = self.experiment_index(value)
            && other != index
        {
            return Err(ExperimentEditError::NameInUse(value.to_string()));
        }
        let path = self.experiment_path(index)?;
        self.set_attribute(&path, name, value)?;
        Ok(())
    }

    /// Sets the commands or condition in an existing `<setup>`, `<go>`, `<exitCondition>` or
    /// other text element of an experiment.
    pub fn set_experiment_text(
        &mut self,
        index: usize,
        part: &str,
        text: &str,
    ) -> Result<(), ExperimentEditError> {
        let path = self.part_path(index, part)?;
        self.set_text(&path, text)?;
        Ok(())
    }

    /// Sets the step limit of an experiment that has a `<timeLimit>` element.
    pub fn set_experiment_time_limit(
        &mut self,
        index: usize,
        steps: u64,
    ) -> Result<(), ExperimentEditError> {
        let path = self.part_path(index, "timeLimit")?;
        self.set_attribute(&path, "steps", &steps.to_string())?;
        Ok(())
    }

    /// Replaces the `<metric>` elements of an experiment.
    pub fn set_experiment_metrics(
        &mut self,
        index: usize,
        metrics: &[String],
    ) -> Result<(), ExperimentEditError> {
        let path = self.part_path(index, "metrics")?;
        self.replace_children(
            &path,
            "metric",
            metrics
                .iter()
                .map(|m| format!("<metric>{}</metric>", encode_content(m, "\n"))),
        )
    }

    /// Removes the children named `name` of the element at `path` and inserts `new` after the
    /// remaining children.
    fn replace_children(
        &mut self,
        path: &[usize],
        name: &str,
        new: impl Iterator<Item = String>,
    ) -> Result<(), ExperimentEditError> {
        let e = self.root().at(path).expect("element path");
        let old: Vec<usize> = e
            .elements()
            .enumerate()
            .filter(|(_, c)| c.name == name)
            .map(|(i, _)| i)
            .collect();
        for i in old.into_iter().rev() {
            self.remove_element(&[path, &[i]].concat())?;
        }
        for xml in new {
            self.insert_element(path, None, &xml)?;
        }
        Ok(())
    }

    /// Sets the values of an enumerated value set. A variable the experiment doesn't list yet
    /// is added at the end of its `<constants>`; a stepped value set becomes an enumerated one
    /// in the same place.
    pub fn set_experiment_values(
        &mut self,
        index: usize,
        variable: &str,
        values: &[String],
    ) -> Result<(), ExperimentEditError> {
        let xml = enumerated_xml(variable, values);
        match self.value_set_path(index, variable)? {
            Some(path) => {
                let e = self.root().at(&path).expect("value set path");
                if e.name == "enumeratedValueSet" {
                    self.replace_children(&path, "value", values.iter().map(|v| value_xml(v)))
                } else {
                    let (parent, i) = path.split_at(path.len() - 1);
                    let parent = parent.to_vec();
                    let i = i[0];
                    self.remove_element(&path)?;
                    let before = (i < self.root().at(&parent).expect("parent").elements().count())
                        .then_some(i);
                    self.insert_element(&parent, before, &xml)?;
                    Ok(())
                }
            }
            None => {
                let constants = self.part_path(index, "constants")?;
                self.insert_element(&constants, None, &xml)?;
                Ok(())
            }
        }
    }

    /// Sets first, step and last of an existing stepped value set.
    pub fn set_experiment_stepped(
        &mut self,
        index: usize,
        variable: &str,
        first: &str,
        step: &str,
        last: &str,
    ) -> Result<(), ExperimentEditError> {
        let path = self
            .value_set_path(index, variable)?
            .filter(|p| {
                self.root()
                    .at(p)
                    .is_some_and(|e| e.name == "steppedValueSet")
            })
            .ok_or_else(|| ExperimentEditError::NoSuchVariable {
                index,
                variable: variable.to_string(),
            })?;
        for (n, v) in [("first", first), ("step", step), ("last", last)] {
            self.set_attribute(&path, n, v)?;
        }
        Ok(())
    }

    /// Removes a variable's value set from an experiment.
    pub fn remove_experiment_variable(
        &mut self,
        index: usize,
        variable: &str,
    ) -> Result<(), ExperimentEditError> {
        let path = self.value_set_path(index, variable)?.ok_or_else(|| {
            ExperimentEditError::NoSuchVariable {
                index,
                variable: variable.to_string(),
            }
        })?;
        self.remove_element(&path)?;
        Ok(())
    }

    fn value_set_path(
        &self,
        index: usize,
        variable: &str,
    ) -> Result<Option<Vec<usize>>, ExperimentEditError> {
        let v = variable.to_string();
        let file = self;
        self.find_in_experiment(index, &move |e: &Element| {
            (e.name == "enumeratedValueSet" || e.name == "steppedValueSet")
                && file
                    .attribute_value(e, "variable")
                    .is_some_and(|n| n.eq_ignore_ascii_case(&v))
        })
    }

    /// Copies an experiment, as written, under a new name right after it. Returns the copy's
    /// index.
    pub fn duplicate_experiment(
        &mut self,
        index: usize,
        new_name: &str,
    ) -> Result<usize, ExperimentEditError> {
        if self.experiment_index(new_name).is_some() {
            return Err(ExperimentEditError::NameInUse(new_name.to_string()));
        }
        let path = self.experiment_path(index)?;
        let e = self.root().at(&path).expect("experiment path");
        // The element's lines as written, with continuation lines' indentation kept relative
        // to the first line.
        let indent = self.indent_of(e.span.start);
        let raw = self.raw(&e.span).replace("\r\n", "\n");
        let xml: String = raw
            .split('\n')
            .enumerate()
            .map(|(i, l)| {
                if i == 0 {
                    l.to_string()
                } else {
                    l.strip_prefix(indent.as_str()).unwrap_or(l).to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let (parent, i) = path.split_at(path.len() - 1);
        let parent = parent.to_vec();
        let count = self.root().at(&parent).expect("parent").elements().count();
        let before = (i[0] + 1 < count).then_some(i[0] + 1);
        self.insert_element(&parent, before, &xml)?;
        let new_index = index + 1;
        let new_path = self.experiment_path(new_index)?;
        self.set_attribute(&new_path, "name", new_name)?;
        Ok(new_index)
    }

    /// Removes an experiment.
    pub fn delete_experiment(&mut self, index: usize) -> Result<(), ExperimentEditError> {
        let path = self.experiment_path(index)?;
        self.remove_element(&path)?;
        Ok(())
    }

    fn indent_of(&self, offset: usize) -> String {
        let text = self.text();
        let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
        let lead = &text[line_start..offset];
        if lead.chars().all(|c| c == ' ' || c == '\t') {
            lead.to_string()
        } else {
            String::new()
        }
    }
}

fn value_xml(v: &str) -> String {
    format!("<value value=\"{}\"></value>", encode_attribute(v, b'"'))
}

fn enumerated_xml(variable: &str, values: &[String]) -> String {
    let mut s = format!(
        "<enumeratedValueSet variable=\"{}\">",
        encode_attribute(variable, b'"')
    );
    for v in values {
        s.push_str("\n  ");
        s.push_str(&value_xml(v));
    }
    s.push_str("\n</enumeratedValueSet>");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<model version="NetLogo 7.0.4" snapToGrid="false">
  <code><![CDATA[to setup end to go end]]></code>
  <widgets>
  </widgets>
  <info><![CDATA[]]></info>
  <experiments>
    <experiment name="E1" repetitions="2" sequentialRunOrder="true" runMetricsEveryStep="false">
      <setup>setup</setup>
      <go>go</go>
      <metrics>
        <metric>count turtles</metric>
      </metrics>
      <constants>
        <enumeratedValueSet variable="a">
          <value value="1"></value>
          <value value="2"></value>
        </enumeratedValueSet>
        <steppedValueSet variable="s" first="0" step="0.1" last="0.3"></steppedValueSet>
        <enumeratedValueSet variable="mode">
          <value value="&quot;fast&quot;"></value>
        </enumeratedValueSet>
      </constants>
    </experiment>
  </experiments>
</model>
"#;

    #[test]
    fn reads_value_sets_and_parts() {
        let f = Nlogox::parse(FILE).unwrap();
        assert_eq!(f.experiment_index("E1"), Some(0));
        assert_eq!(
            f.experiment_parts(0),
            ["setup", "go", "metrics", "constants"]
        );
        let v = f.experiment_value_sets(0);
        assert_eq!(v.len(), 3);
        assert_eq!(
            v[2],
            ValueSet::Enumerated {
                variable: "mode".into(),
                values: vec!["\"fast\"".into()]
            }
        );
        assert!(matches!(&v[1], ValueSet::Stepped { last, .. } if last == "0.3"));
    }

    #[test]
    fn edits_round_trip_to_the_original_bytes() {
        let mut f = Nlogox::parse(FILE).unwrap();
        f.set_experiment_attribute(0, "repetitions", "5").unwrap();
        f.set_experiment_text(0, "setup", "setup\nreset-ticks")
            .unwrap();
        f.set_experiment_metrics(
            0,
            &[
                "count turtles".into(),
                "mean [x] of patches with [x < 3]".into(),
            ],
        )
        .unwrap();
        f.set_experiment_values(0, "a", &["1".into(), "2".into(), "3".into()])
            .unwrap();
        f.set_experiment_stepped(0, "s", "0", "0.05", "0.3")
            .unwrap();
        assert!(f.text().contains(r#"<value value="3"></value>"#));
        assert!(f.text().contains(r#"step="0.05""#));
        assert!(
            f.text()
                .contains("<metric><![CDATA[mean [x] of patches with [x < 3]]]></metric>")
        );
        // Undo each edit by hand: back to the same bytes.
        f.set_experiment_attribute(0, "repetitions", "2").unwrap();
        f.set_experiment_text(0, "setup", "setup").unwrap();
        f.set_experiment_metrics(0, &["count turtles".into()])
            .unwrap();
        f.set_experiment_values(0, "a", &["1".into(), "2".into()])
            .unwrap();
        f.set_experiment_stepped(0, "s", "0", "0.1", "0.3").unwrap();
        assert_eq!(f.text(), FILE);
    }

    #[test]
    fn add_and_remove_variables() {
        let mut f = Nlogox::parse(FILE).unwrap();
        f.set_experiment_values(0, "b", &["true".into(), "false".into()])
            .unwrap();
        let v = f.experiment_value_sets(0);
        assert_eq!(v.last().unwrap().variable(), "b");
        assert!(f.text().contains(
            "        <enumeratedValueSet variable=\"b\">\n          <value value=\"true\"></value>"
        ));
        f.remove_experiment_variable(0, "b").unwrap();
        assert_eq!(f.text(), FILE);
        // A stepped set replaced by values stays in its place.
        f.set_experiment_values(0, "s", &["0".into(), "1".into()])
            .unwrap();
        assert_eq!(f.experiment_value_sets(0)[1].variable(), "s");
        assert!(f.remove_experiment_variable(0, "zzz").is_err());
    }

    #[test]
    fn duplicate_rename_and_delete() {
        let mut f = Nlogox::parse(FILE).unwrap();
        assert_eq!(
            f.duplicate_experiment(0, "E1").unwrap_err(),
            ExperimentEditError::NameInUse("E1".into())
        );
        let i = f.duplicate_experiment(0, "E2").unwrap();
        assert_eq!(i, 1);
        assert_eq!(f.experiment_index("E2"), Some(1));
        assert_eq!(f.experiment_value_sets(1).len(), 3);
        assert!(
            f.text()
                .contains("    </experiment>\n    <experiment name=\"E2\"")
        );
        assert!(f.set_experiment_attribute(1, "name", "E1").is_err());
        f.delete_experiment(1).unwrap();
        assert_eq!(f.text(), FILE);
        assert!(matches!(
            f.set_experiment_time_limit(0, 10),
            Err(ExperimentEditError::MissingPart { .. })
        ));
    }

    #[test]
    fn crlf_files_stay_crlf() {
        let crlf = FILE.replace('\n', "\r\n");
        let mut f = Nlogox::parse(crlf.clone()).unwrap();
        f.duplicate_experiment(0, "E2").unwrap();
        f.set_experiment_values(0, "b", &["1".into()]).unwrap();
        assert!(!f.text().replace("\r\n", "").contains('\n'));
        f.remove_experiment_variable(0, "b").unwrap();
        f.delete_experiment(1).unwrap();
        assert_eq!(f.text(), crlf);
    }
}
