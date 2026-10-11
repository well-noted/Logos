//! Experiment results as Parquet (Part E, E2).
//!
//! One file per experiment, one row per measurement, with the same columns as BehaviorSpace's
//! table output: `[run number]`, the variables, `[step]`, the metrics. Column types come from
//! the values:
//!
//! - numbers are doubles, as in NetLogo; `[run number]` and `[step]` are 64-bit integers;
//! - booleans and strings keep their types;
//! - lists become Parquet lists, nested as deep as the values are, when every list in the
//!   column holds the same kind of item;
//! - a column whose values don't share one of these types (a number in some rows and a list in
//!   others, or agents) holds the text BehaviorSpace writes for each value.
//!
//! A metric that failed in a run is null in that row; the run's errors are in its run record.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use ::parquet::arrow::ArrowWriter;
use ::parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use ::parquet::basic::Compression;
use ::parquet::file::properties::WriterProperties;
use arrow_array::{
    Array, ArrayRef, BooleanArray, Float64Array, Int64Array, ListArray, RecordBatch, StringArray,
};
use arrow_buffer::{NullBuffer, OffsetBuffer};
use arrow_schema::{DataType, Field, Schema};
use logos_bridge_client::protocol::{Cell, LabRunResult, LoadedExperiment, NlValue};

#[derive(Debug, thiserror::Error)]
pub enum ParquetError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Parquet(#[from] ::parquet::errors::ParquetError),
    #[error(transparent)]
    Arrow(#[from] arrow_schema::ArrowError),
    #[error("{0}")]
    Format(String),
}

/// The type of a column, found from its values.
#[derive(Debug, Clone, PartialEq)]
pub enum ColumnType {
    Number,
    Boolean,
    String,
    /// A list; `None` while every list seen so far was empty.
    List(Option<Box<ColumnType>>),
    /// BehaviorSpace's text for each value.
    Text,
}

impl ColumnType {
    fn of(v: &NlValue) -> ColumnType {
        match v {
            NlValue::Number(_) => ColumnType::Number,
            NlValue::Boolean(_) => ColumnType::Boolean,
            NlValue::String(_) => ColumnType::String,
            NlValue::List(items) => {
                let mut inner: Option<ColumnType> = None;
                for item in items {
                    let t = ColumnType::of(item);
                    inner = Some(match inner {
                        None => t,
                        Some(prev) => prev.unify(t),
                    });
                }
                match inner {
                    Some(ColumnType::Text) => ColumnType::Text,
                    i => ColumnType::List(i.map(Box::new)),
                }
            }
            _ => ColumnType::Text,
        }
    }

    fn unify(self, other: ColumnType) -> ColumnType {
        match (self, other) {
            (a, b) if a == b => a,
            (ColumnType::List(None), ColumnType::List(b)) => ColumnType::List(b),
            (ColumnType::List(a), ColumnType::List(None)) => ColumnType::List(a),
            (ColumnType::List(Some(a)), ColumnType::List(Some(b))) => match a.unify(*b) {
                ColumnType::Text => ColumnType::Text,
                t => ColumnType::List(Some(Box::new(t))),
            },
            _ => ColumnType::Text,
        }
    }

    fn data_type(&self) -> DataType {
        match self {
            ColumnType::Number => DataType::Float64,
            ColumnType::Boolean => DataType::Boolean,
            ColumnType::String | ColumnType::Text => DataType::Utf8,
            ColumnType::List(inner) => DataType::List(Arc::new(Field::new_list_field(
                inner
                    .as_ref()
                    .map(|t| t.data_type())
                    .unwrap_or(DataType::Float64),
                true,
            ))),
        }
    }
}

/// The value a cell contributes to a typed column; `None` for a failed metric or an unset
/// variable.
fn value(cell: &Cell) -> Option<&NlValue> {
    match &cell.value {
        Ok(NlValue::Other(_)) | Err(_) => None,
        Ok(v) => Some(v),
    }
}

/// The type for a column holding these cells. A column with no values at all is text.
pub fn column_type<'a>(cells: impl IntoIterator<Item = &'a Cell>) -> ColumnType {
    let mut t: Option<ColumnType> = None;
    for c in cells {
        // An unset variable or a failed metric doesn't decide the type.
        let Some(v) = value(c) else {
            continue;
        };
        let vt = ColumnType::of(v);
        t = Some(match t {
            None => vt,
            Some(prev) => prev.unify(vt),
        });
        if t == Some(ColumnType::Text) {
            break;
        }
    }
    t.unwrap_or(ColumnType::Text)
}

fn build(ty: &ColumnType, values: &[Option<&NlValue>]) -> ArrayRef {
    match ty {
        ColumnType::Number => Arc::new(Float64Array::from(
            values
                .iter()
                .map(|v| match v {
                    Some(NlValue::Number(n)) => Some(*n),
                    _ => None,
                })
                .collect::<Vec<_>>(),
        )),
        ColumnType::Boolean => Arc::new(BooleanArray::from(
            values
                .iter()
                .map(|v| match v {
                    Some(NlValue::Boolean(b)) => Some(*b),
                    _ => None,
                })
                .collect::<Vec<_>>(),
        )),
        ColumnType::String => Arc::new(StringArray::from(
            values
                .iter()
                .map(|v| match v {
                    Some(NlValue::String(s)) => Some(s.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
        )),
        ColumnType::List(inner) => {
            let inner_ty = inner.as_deref().cloned().unwrap_or(ColumnType::Number);
            let mut offsets = vec![0i32];
            let mut items: Vec<Option<&NlValue>> = Vec::new();
            let mut valid = Vec::with_capacity(values.len());
            for v in values {
                if let Some(NlValue::List(xs)) = v {
                    items.extend(xs.iter().map(Some));
                    valid.push(true);
                } else {
                    valid.push(false);
                }
                offsets.push(items.len() as i32);
            }
            let child = build(&inner_ty, &items);
            let field = Arc::new(Field::new_list_field(inner_ty.data_type(), true));
            Arc::new(
                ListArray::try_new(
                    field,
                    OffsetBuffer::new(offsets.into()),
                    child,
                    Some(NullBuffer::from(valid)),
                )
                .expect("offsets and child are built together"),
            )
        }
        ColumnType::Text => unreachable!("text columns are built from the cells' text"),
    }
}

/// One column of the output: its name, type and cells.
struct Column<'a> {
    name: String,
    cells: Vec<&'a Cell>,
}

/// Column names as in table output, made unique (a metric may appear twice) by adding
/// ` (2)`, ` (3)` and so on.
fn unique_names(names: Vec<String>) -> Vec<String> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    names
        .into_iter()
        .map(|n| {
            let count = seen.entry(n.clone()).or_insert(0);
            *count += 1;
            if *count == 1 {
                n
            } else {
                format!("{n} ({count})")
            }
        })
        .collect()
}

/// Metadata stored in the file's key-value metadata.
pub struct FileInfo<'a> {
    pub model_path: &'a str,
    pub netlogo_version: &'a str,
}

/// Writes the measurements of `runs` (in the order given) to a Parquet file.
pub fn write_parquet(
    path: &Path,
    experiment: &LoadedExperiment,
    runs: &[&LabRunResult],
    info: &FileInfo,
) -> Result<(), ParquetError> {
    // Rows: one per measurement with values; an experiment without metrics has one row per
    // completed run, as in table output.
    let mut run_numbers = Vec::new();
    let mut steps = Vec::new();
    let mut settings: Vec<&[logos_bridge_client::protocol::Setting]> = Vec::new();
    let mut metric_cells: Vec<&[Cell]> = Vec::new();
    for r in runs {
        if experiment.metrics.is_empty() {
            if r.reason == logos_bridge_client::protocol::LabRunEnd::Completed {
                run_numbers.push(r.run_number as i64);
                steps.push(r.steps as i64);
                settings.push(&r.settings);
                metric_cells.push(&[]);
            }
            continue;
        }
        for m in &r.measurements {
            if m.values.is_empty() {
                continue;
            }
            if m.values.len() != experiment.metrics.len() {
                return Err(ParquetError::Format(format!(
                    "run {} has {} values for {} metrics",
                    r.run_number,
                    m.values.len(),
                    experiment.metrics.len()
                )));
            }
            run_numbers.push(r.run_number as i64);
            steps.push(m.step as i64);
            settings.push(&r.settings);
            metric_cells.push(&m.values);
        }
    }

    let mut columns: Vec<Column> = Vec::new();
    for (i, name) in experiment.variables.iter().enumerate() {
        columns.push(Column {
            name: name.clone(),
            cells: settings.iter().map(|s| &s[i].cell).collect(),
        });
    }
    let first_metric = columns.len();
    for (i, name) in experiment.metrics.iter().enumerate() {
        columns.push(Column {
            name: name.clone(),
            cells: metric_cells.iter().map(|m| &m[i]).collect(),
        });
    }

    let mut names = vec!["[run number]".to_owned()];
    names.extend(columns[..first_metric].iter().map(|c| c.name.clone()));
    names.push("[step]".to_owned());
    names.extend(columns[first_metric..].iter().map(|c| c.name.clone()));
    let names = unique_names(names);

    let mut fields = Vec::new();
    let mut arrays: Vec<ArrayRef> = Vec::new();
    let mut push = |ty: DataType, array: ArrayRef| {
        let name = &names[fields.len()];
        fields.push(Field::new(name, ty, true));
        arrays.push(array);
    };
    push(
        DataType::Int64,
        Arc::new(Int64Array::from(run_numbers.clone())),
    );
    let column = |c: &Column| {
        let ty = column_type(c.cells.iter().copied());
        let array: ArrayRef = if ty == ColumnType::Text {
            Arc::new(StringArray::from(
                c.cells
                    .iter()
                    .map(|cell| value(cell).map(|_| cell.text.as_str()))
                    .collect::<Vec<_>>(),
            ))
        } else {
            let values: Vec<Option<&NlValue>> = c.cells.iter().map(|cell| value(cell)).collect();
            build(&ty, &values)
        };
        (ty.data_type(), array)
    };
    for c in &columns[..first_metric] {
        let (t, a) = column(c);
        push(t, a);
    }
    push(DataType::Int64, Arc::new(Int64Array::from(steps)));
    for c in &columns[first_metric..] {
        let (t, a) = column(c);
        push(t, a);
    }

    let mut metadata = std::collections::HashMap::new();
    metadata.insert("logos.experiment".to_owned(), experiment.name.clone());
    metadata.insert("logos.model".to_owned(), info.model_path.to_owned());
    metadata.insert(
        "logos.netlogoVersion".to_owned(),
        info.netlogo_version.to_owned(),
    );
    let schema = Arc::new(Schema::new_with_metadata(fields, metadata));
    let batch = RecordBatch::try_new(schema.clone(), arrays)?;

    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    // Write next to the destination and rename, so a crash never leaves half a file.
    let partial = path.with_extension("parquet.partial");
    let file = std::fs::File::create(&partial)?;
    let mut writer = ArrowWriter::try_new(file, schema, Some(props))?;
    writer.write(&batch)?;
    writer.close()?;
    std::fs::rename(&partial, path)?;
    Ok(())
}

/// A Parquet file read back: column names and, per row, each column's value (`None` for
/// null). Typed columns give typed values; text columns give `NlValue::String` of the text.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadBack {
    pub columns: Vec<String>,
    pub types: Vec<DataType>,
    pub rows: Vec<Vec<Option<NlValue>>>,
}

fn to_value(array: &dyn Array, i: usize) -> Option<NlValue> {
    if array.is_null(i) {
        return None;
    }
    let any = array.as_any();
    if let Some(a) = any.downcast_ref::<Float64Array>() {
        return Some(NlValue::Number(a.value(i)));
    }
    if let Some(a) = any.downcast_ref::<Int64Array>() {
        return Some(NlValue::Number(a.value(i) as f64));
    }
    if let Some(a) = any.downcast_ref::<BooleanArray>() {
        return Some(NlValue::Boolean(a.value(i)));
    }
    if let Some(a) = any.downcast_ref::<StringArray>() {
        return Some(NlValue::String(a.value(i).to_owned()));
    }
    if let Some(a) = any.downcast_ref::<ListArray>() {
        let items = a.value(i);
        return Some(NlValue::List(
            (0..items.len())
                .map(|j| to_value(items.as_ref(), j).unwrap_or(NlValue::Nobody))
                .collect(),
        ));
    }
    None
}

/// Reads a file written by [`write_parquet`].
pub fn read_parquet(path: &Path) -> Result<ReadBack, ParquetError> {
    let file = std::fs::File::open(path)?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?.build()?;
    let mut out = ReadBack {
        columns: Vec::new(),
        types: Vec::new(),
        rows: Vec::new(),
    };
    for batch in reader {
        let batch = batch?;
        if out.columns.is_empty() {
            for f in batch.schema().fields() {
                out.columns.push(f.name().clone());
                out.types.push(f.data_type().clone());
            }
        }
        for i in 0..batch.num_rows() {
            out.rows.push(
                batch
                    .columns()
                    .iter()
                    .map(|c| to_value(c.as_ref(), i))
                    .collect(),
            );
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use logos_bridge_client::protocol::{
        LabError, LabRunEnd, Measurement, Setting, WorldDimensions,
    };

    fn num(n: f64) -> NlValue {
        NlValue::Number(n)
    }

    fn cell(v: NlValue) -> Cell {
        Cell {
            text: format!("{v:?}"),
            value: Ok(v),
        }
    }

    fn failed() -> Cell {
        Cell {
            value: Err(LabError {
                kind: "RuntimePrimitiveException".into(),
                message: "boom".into(),
            }),
            text: "<RuntimePrimitiveException>".into(),
        }
    }

    #[test]
    fn column_types_follow_the_values() {
        let t = |vs: Vec<NlValue>| column_type(vs.into_iter().map(cell).collect::<Vec<_>>().iter());
        assert_eq!(t(vec![num(1.0), num(2.5)]), ColumnType::Number);
        assert_eq!(
            t(vec![NlValue::List(vec![]), NlValue::List(vec![num(1.0)])]),
            ColumnType::List(Some(Box::new(ColumnType::Number)))
        );
        assert_eq!(
            t(vec![NlValue::List(vec![NlValue::List(vec![num(1.0)])])]),
            ColumnType::List(Some(Box::new(ColumnType::List(Some(Box::new(
                ColumnType::Number
            ))))))
        );
        assert_eq!(
            t(vec![num(0.0), NlValue::List(vec![num(1.0)])]),
            ColumnType::Text
        );
        assert_eq!(
            t(vec![NlValue::List(vec![
                num(1.0),
                NlValue::String("a".into())
            ])]),
            ColumnType::Text
        );
        assert_eq!(t(vec![NlValue::Turtle(3)]), ColumnType::Text);
        // A failed metric doesn't change the type.
        assert_eq!(
            column_type([&cell(num(1.0)), &failed()]),
            ColumnType::Number
        );
    }

    #[test]
    fn writes_and_reads_back() {
        let experiment = LoadedExperiment {
            id: 1,
            name: "E".into(),
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
            metrics: vec!["n".into(), "xs".into(), "n".into(), "mixed".into()],
            variables: vec!["mode".into()],
            runs: Vec::new(),
            world_dimensions: WorldDimensions {
                min_pxcor: 0,
                max_pxcor: 0,
                min_pycor: 0,
                max_pycor: 0,
            },
        };
        let run = LabRunResult {
            run_number: 4,
            settings: vec![Setting {
                name: "mode".into(),
                cell: cell(NlValue::String("zones".into())),
            }],
            reason: LabRunEnd::Completed,
            steps: 1,
            measurements: vec![
                Measurement {
                    step: 0,
                    values: vec![
                        cell(num(1.0)),
                        cell(NlValue::List(vec![])),
                        failed(),
                        cell(num(0.0)),
                    ],
                },
                Measurement {
                    step: 1,
                    values: vec![
                        cell(num(2.5)),
                        cell(NlValue::List(vec![num(1.0), num(2.0)])),
                        cell(num(3.0)),
                        Cell {
                            value: Ok(NlValue::List(vec![num(1.0), num(2.0)])),
                            text: "[1 2]".into(),
                        },
                    ],
                },
            ],
            errors: Vec::new(),
            elapsed_millis: 0.0,
            raw: serde_json::Value::Null,
        };
        let dir = std::env::temp_dir().join(format!("logos-parquet-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("e.parquet");
        write_parquet(
            &path,
            &experiment,
            &[&run],
            &FileInfo {
                model_path: "m.nlogox",
                netlogo_version: "7.0.4",
            },
        )
        .unwrap();
        let back = read_parquet(&path).unwrap();
        assert_eq!(
            back.columns,
            vec![
                "[run number]",
                "mode",
                "[step]",
                "n",
                "xs",
                "n (2)",
                "mixed"
            ]
        );
        assert_eq!(back.types[3], DataType::Float64);
        assert!(matches!(back.types[4], DataType::List(_)));
        assert_eq!(back.types[6], DataType::Utf8);
        assert_eq!(back.rows.len(), 2);
        assert_eq!(back.rows[0][0], Some(num(4.0)));
        assert_eq!(back.rows[0][1], Some(NlValue::String("zones".into())));
        assert_eq!(
            back.rows[1][4],
            Some(NlValue::List(vec![num(1.0), num(2.0)]))
        );
        assert_eq!(back.rows[0][4], Some(NlValue::List(vec![])));
        assert_eq!(back.rows[0][5], None, "failed metric");
        assert_eq!(back.rows[1][6], Some(NlValue::String("[1 2]".into())));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
