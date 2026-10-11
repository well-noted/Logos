//! BehaviorSpace "table" output (CSV, table version 2.0): reading it, and writing it the way
//! NetLogo 7.0.4's headless BehaviorSpace does.
//!
//! The file starts with six metadata lines (format, model path, experiment name, date, world
//! dimension names, world dimension values), then a header row and one row per measurement.

use std::io::Write;

use logos_bridge_client::protocol::{LabRunEnd, LabRunResult, LoadedExperiment};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TableError {
    #[error("not a BehaviorSpace table: {0}")]
    Format(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// The first metadata line, for example `BehaviorSpace results (NetLogo 7.0.4)`.
    pub producer: String,
    pub experiment: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl Table {
    pub fn parse(text: &str) -> Result<Table, TableError> {
        let records = parse_csv(text);
        if records.len() < 7 {
            return Err(TableError::Format("fewer than 7 lines".into()));
        }
        let producer = records[0].first().cloned().unwrap_or_default();
        if !producer.starts_with("BehaviorSpace results") {
            return Err(TableError::Format(format!("first line is `{producer}`")));
        }
        let experiment = records[2].first().cloned().unwrap_or_default();
        let columns = records[6].clone();
        let rows: Vec<Vec<String>> = records[7..]
            .iter()
            .filter(|r| !(r.len() == 1 && r[0].is_empty()))
            .cloned()
            .collect();
        for (i, r) in rows.iter().enumerate() {
            if r.len() != columns.len() {
                return Err(TableError::Format(format!(
                    "row {} has {} cells, the header has {}",
                    i + 1,
                    r.len(),
                    columns.len()
                )));
            }
        }
        Ok(Table {
            producer,
            experiment,
            columns,
            rows,
        })
    }

    pub fn column(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c == name)
    }

    /// The last row recorded for a run number (with metrics measured only at the end, the
    /// only row).
    pub fn final_row(&self, run_number: u64) -> Option<&[String]> {
        let col = self.column("[run number]")?;
        let wanted = run_number.to_string();
        self.rows
            .iter()
            .filter(|r| r[col] == wanted)
            .max_by_key(|r| {
                self.column("[step]")
                    .and_then(|s| r[s].parse::<u64>().ok())
                    .unwrap_or(0)
            })
            .map(Vec::as_slice)
    }
}

/// The line ending NetLogo's output uses on this platform (Java's `println`).
pub const LINE_END: &str = if cfg!(windows) { "\r\n" } else { "\n" };

/// Quotes a cell the way NetLogo's CSV writer does: always quoted, quotes doubled.
pub fn csv_cell(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn csv_row<S: AsRef<str>>(cells: impl IntoIterator<Item = S>) -> String {
    cells
        .into_iter()
        .map(|c| csv_cell(c.as_ref()))
        .collect::<Vec<_>>()
        .join(",")
}

/// The six metadata lines that start table and spreadsheet output.
pub fn export_header(
    kind: &str,
    netlogo_version: &str,
    model_path: &str,
    experiment: &LoadedExperiment,
    date: &str,
) -> String {
    let version = if netlogo_version.starts_with("NetLogo") {
        netlogo_version.to_owned()
    } else {
        format!("NetLogo {netlogo_version}")
    };
    let d = experiment.world_dimensions;
    [
        csv_row([
            format!("BehaviorSpace results ({version})"),
            format!("{kind} version 2.0"),
        ]),
        csv_cell(model_path),
        csv_cell(&experiment.name),
        csv_cell(date),
        csv_row(["min-pxcor", "max-pxcor", "min-pycor", "max-pycor"]),
        csv_row([d.min_pxcor, d.max_pxcor, d.min_pycor, d.max_pycor].map(|n| n.to_string())),
    ]
    .iter()
    .map(|l| format!("{l}{LINE_END}"))
    .collect()
}

/// The current local time as BehaviorSpace writes it (`MM/dd/yyyy HH:mm:ss:SSS Z`).
pub fn export_date() -> String {
    chrono::Local::now()
        .format("%m/%d/%Y %H:%M:%S:%3f %z")
        .to_string()
}

/// Writes table output as runs finish. Rows appear in the order runs are written, as in
/// NetLogo, where parallel runs write in the order they finish.
pub struct TableWriter<W: Write> {
    out: W,
    metrics: usize,
}

impl<W: Write> TableWriter<W> {
    /// Starts a new table: metadata lines and the column header.
    pub fn new(
        mut out: W,
        netlogo_version: &str,
        model_path: &str,
        experiment: &LoadedExperiment,
    ) -> std::io::Result<TableWriter<W>> {
        out.write_all(
            export_header(
                "Table",
                netlogo_version,
                model_path,
                experiment,
                &export_date(),
            )
            .as_bytes(),
        )?;
        let header = std::iter::once("[run number]".to_owned())
            .chain(experiment.variables.iter().cloned())
            .chain(std::iter::once("[step]".to_owned()))
            .chain(experiment.metrics.iter().cloned());
        write!(out, "{}{LINE_END}", csv_row(header))?;
        out.flush()?;
        Ok(TableWriter {
            out,
            metrics: experiment.metrics.len(),
        })
    }

    /// Continues a table written earlier (resume): nothing is written until the next run.
    pub fn append(out: W, experiment: &LoadedExperiment) -> TableWriter<W> {
        TableWriter {
            out,
            metrics: experiment.metrics.len(),
        }
    }

    /// Writes the rows of one run: one per measurement, or, for an experiment without metrics,
    /// one recording the run's length when it completed.
    pub fn write_run(&mut self, run: &LabRunResult) -> std::io::Result<()> {
        let mut text = String::new();
        let prefix = |step: u64| {
            std::iter::once(run.run_number.to_string())
                .chain(run.settings.iter().map(|s| s.cell.text.clone()))
                .chain(std::iter::once(step.to_string()))
        };
        for m in &run.measurements {
            if m.values.is_empty() {
                continue;
            }
            let row = prefix(m.step).chain(m.values.iter().map(|c| c.text.clone()));
            text.push_str(&csv_row(row));
            text.push_str(LINE_END);
        }
        if self.metrics == 0 && run.reason == LabRunEnd::Completed {
            text.push_str(&csv_row(prefix(run.steps)));
            text.push_str(LINE_END);
        }
        self.out.write_all(text.as_bytes())?;
        self.out.flush()
    }

    pub fn into_inner(self) -> W {
        self.out
    }
}

/// Minimal RFC 4180 reader: quoted fields, doubled quotes, CRLF or LF line ends.
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => in_quotes = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' => in_quotes = true,
            ',' => record.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\"BehaviorSpace results (NetLogo 7.0.4)\",\"Table version 2.0\"\r\n\
\"C:\\model.nlogox\"\r\n\
\"E1\"\r\n\
\"10/09/2026 14:12:10:343 -0700\"\r\n\
\"min-pxcor\",\"max-pxcor\"\r\n\
\"0\",\"79\"\r\n\
\"[run number]\",\"seed\",\"[step]\",\"count turtles\",\"label\"\r\n\
\"2\",\"2\",\"551\",\"13\",\"say \"\"hi\"\", ok\"\r\n\
\"1\",\"1\",\"551\",\"[1 2]\",\"x\"\r\n";

    #[test]
    fn reads_metadata_header_and_rows() {
        let t = Table::parse(SAMPLE).unwrap();
        assert_eq!(t.producer, "BehaviorSpace results (NetLogo 7.0.4)");
        assert_eq!(t.experiment, "E1");
        assert_eq!(t.columns.len(), 5);
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0][4], "say \"hi\", ok");
        assert_eq!(t.final_row(1).unwrap()[3], "[1 2]");
        assert!(t.final_row(3).is_none());
    }

    fn experiment(metrics: &[&str]) -> LoadedExperiment {
        use logos_bridge_client::protocol::WorldDimensions;
        LoadedExperiment {
            id: 1,
            name: "E \"1\"".into(),
            repetitions: 1,
            sequential_run_order: true,
            run_metrics_every_step: true,
            run_metrics_condition: String::new(),
            time_limit: 0,
            exit_condition: String::new(),
            pre_experiment: String::new(),
            setup: String::new(),
            go: String::new(),
            post_run: String::new(),
            post_experiment: String::new(),
            metrics: metrics.iter().map(|m| m.to_string()).collect(),
            variables: vec!["mode".into()],
            runs: Vec::new(),
            world_dimensions: WorldDimensions {
                min_pxcor: -5,
                max_pxcor: 5,
                min_pycor: 0,
                max_pycor: 9,
            },
        }
    }

    fn run(reason: LabRunEnd, measurements: &[(u64, &[&str])]) -> LabRunResult {
        use logos_bridge_client::protocol::{Cell, Measurement, NlValue, Setting};
        let cell = |t: &str| Cell {
            value: Ok(NlValue::Other(t.into())),
            text: t.into(),
        };
        LabRunResult {
            run_number: 2,
            settings: vec![Setting {
                name: "mode".into(),
                cell: cell("zones"),
            }],
            reason,
            steps: 7,
            measurements: measurements
                .iter()
                .map(|(step, values)| Measurement {
                    step: *step,
                    values: values.iter().map(|v| cell(v)).collect(),
                })
                .collect(),
            errors: Vec::new(),
            elapsed_millis: 0.0,
        }
    }

    #[test]
    fn writes_what_it_reads() {
        let e = experiment(&["count turtles", "label"]);
        let mut w = TableWriter::new(Vec::new(), "7.0.4", "C:\\m.nlogox", &e).unwrap();
        w.write_run(&run(
            LabRunEnd::Completed,
            &[(0, &["0", "a"]), (7, &["13", "say \"hi\""])],
        ))
        .unwrap();
        let text = String::from_utf8(w.into_inner()).unwrap();
        let lines: Vec<&str> = text.split(LINE_END).collect();
        assert_eq!(
            lines[0],
            "\"BehaviorSpace results (NetLogo 7.0.4)\",\"Table version 2.0\""
        );
        assert_eq!(lines[2], "\"E \"\"1\"\"\"");
        assert_eq!(lines[5], "\"-5\",\"5\",\"0\",\"9\"");
        assert_eq!(
            lines[6],
            "\"[run number]\",\"mode\",\"[step]\",\"count turtles\",\"label\""
        );
        assert_eq!(lines[8], "\"2\",\"zones\",\"7\",\"13\",\"say \"\"hi\"\"\"");
        let t = Table::parse(&text).unwrap();
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[1][4], "say \"hi\"");
    }

    #[test]
    fn an_experiment_without_metrics_records_completed_runs_only() {
        let e = experiment(&[]);
        let mut w = TableWriter::append(Vec::new(), &e);
        w.write_run(&run(LabRunEnd::Completed, &[(0, &[]), (7, &[])]))
            .unwrap();
        w.write_run(&run(LabRunEnd::Halted, &[])).unwrap();
        let text = String::from_utf8(w.into_inner()).unwrap();
        assert_eq!(text, format!("\"2\",\"zones\",\"7\"{LINE_END}"));
    }

    #[test]
    fn rejects_other_files() {
        assert!(Table::parse("a,b\n1,2\n").is_err());
    }
}
