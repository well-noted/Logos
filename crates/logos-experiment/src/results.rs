//! A results folder: everything one `logos run` of an experiment writes, kept so that an
//! interrupted experiment can be resumed (E4).
//!
//! ```text
//! experiment.json     what was run: experiment, model, setup file, engine, number of runs
//! runs/000001.json    one file per finished run: its run record and the bridge's result
//! table.csv           BehaviorSpace table output, written as runs finish
//! results.parquet     all measurements, written when every run has finished
//! ```
//!
//! Run files are written whole (to a temporary name, then renamed), so a run file that exists
//! is complete. Runs stopped partway (halted) are not saved; resuming runs them again.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use logos_bridge_client::protocol::{LabRunEnd, LabRunResult, LoadedExperiment};
use serde::{Deserialize, Serialize};

use crate::parquet::{FileInfo, ParquetError, write_parquet};
use crate::provenance::{Provenance, RunRecord};
use crate::table::TableWriter;

pub const EXPERIMENT_FILE: &str = "experiment.json";
pub const RUNS_DIR: &str = "runs";
pub const TABLE_FILE: &str = "table.csv";
pub const PARQUET_FILE: &str = "results.parquet";

#[derive(Debug, thiserror::Error)]
pub enum ResultsError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: {detail}")]
    Format { path: PathBuf, detail: String },
    #[error("{0}")]
    Mismatch(String),
    #[error(transparent)]
    Parquet(#[from] ParquetError),
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> ResultsError + '_ {
    move |source| ResultsError::Io {
        path: path.to_owned(),
        source,
    }
}

/// What was run, saved in `experiment.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunMeta {
    pub experiment: String,
    /// Absolute path of the model file.
    pub model: PathBuf,
    /// The project folder (where `logos.toml` is or would be).
    pub project: PathBuf,
    /// The separate experiments file, if the experiment isn't stored in the model.
    pub setup_file: Option<PathBuf>,
    pub netlogo_version: String,
    pub run_count: u64,
    pub variables: Vec<String>,
    pub metrics: Vec<String>,
    pub update_plots: bool,
    /// When the folder was created (local time, RFC 3339).
    pub created: String,
    /// The engine and files the experiment started with (E3).
    #[serde(default)]
    pub provenance: Option<Provenance>,
}

impl RunMeta {
    /// Checks that a loaded experiment is the one these results were started with.
    pub fn check(&self, e: &LoadedExperiment) -> Result<(), ResultsError> {
        let mut changed = Vec::new();
        if e.runs.len() as u64 != self.run_count {
            changed.push(format!(
                "it has {} runs, the results have {}",
                e.runs.len(),
                self.run_count
            ));
        }
        if e.variables != self.variables {
            changed.push("its variables changed".into());
        }
        if e.metrics != self.metrics {
            changed.push("its metrics changed".into());
        }
        if changed.is_empty() {
            Ok(())
        } else {
            Err(ResultsError::Mismatch(format!(
                "experiment {} changed since these results were started: {}",
                self.experiment,
                changed.join("; ")
            )))
        }
    }
}

/// A saved run: the bridge's result and, for runs saved since run records exist, the record.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedRun {
    pub result: LabRunResult,
    pub record: Option<RunRecord>,
}

pub struct ResultsDir {
    dir: PathBuf,
    pub meta: RunMeta,
}

impl ResultsDir {
    /// Creates a new results folder. Fails if it already holds results.
    pub fn create(dir: &Path, meta: RunMeta) -> Result<ResultsDir, ResultsError> {
        let file = dir.join(EXPERIMENT_FILE);
        if file.exists() {
            return Err(ResultsError::Mismatch(format!(
                "{} already holds results; resume them with --resume or choose another folder",
                dir.display()
            )));
        }
        std::fs::create_dir_all(dir.join(RUNS_DIR)).map_err(io(dir))?;
        let text = serde_json::to_string_pretty(&meta).expect("metadata serializes");
        write_whole(&file, text.as_bytes())?;
        Ok(ResultsDir {
            dir: dir.to_owned(),
            meta,
        })
    }

    /// Opens an existing results folder.
    pub fn open(dir: &Path) -> Result<ResultsDir, ResultsError> {
        let file = dir.join(EXPERIMENT_FILE);
        let text = std::fs::read_to_string(&file).map_err(io(&file))?;
        let meta = serde_json::from_str(&text).map_err(|e| ResultsError::Format {
            path: file.clone(),
            detail: e.to_string(),
        })?;
        std::fs::create_dir_all(dir.join(RUNS_DIR)).map_err(io(dir))?;
        Ok(ResultsDir {
            dir: dir.to_owned(),
            meta,
        })
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }

    fn run_file(&self, run_number: u64) -> PathBuf {
        self.dir
            .join(RUNS_DIR)
            .join(format!("{run_number:06}.json"))
    }

    /// Saves a finished run with its record. Halted runs are not saved.
    pub fn save_run(&self, run: &LabRunResult, record: &RunRecord) -> Result<bool, ResultsError> {
        if run.reason == LabRunEnd::Halted {
            return Ok(false);
        }
        let path = self.run_file(run.run_number);
        let file = serde_json::json!({ "record": record, "result": run.raw });
        let text = serde_json::to_vec_pretty(&file).expect("JSON serializes");
        write_whole(&path, &text)?;
        Ok(true)
    }

    /// The runs saved so far, in run-number order.
    pub fn saved_runs(&self) -> Result<Vec<LabRunResult>, ResultsError> {
        Ok(self.saved()?.into_iter().map(|s| s.result).collect())
    }

    /// The runs saved so far with their records, in run-number order.
    pub fn saved(&self) -> Result<Vec<SavedRun>, ResultsError> {
        let runs_dir = self.dir.join(RUNS_DIR);
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&runs_dir).map_err(io(&runs_dir))? {
            let path = entry.map_err(io(&runs_dir))?.path();
            if path.extension().is_none_or(|x| x != "json") {
                continue;
            }
            let text = std::fs::read_to_string(&path).map_err(io(&path))?;
            let format = |detail: String| ResultsError::Format {
                path: path.clone(),
                detail,
            };
            let json: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| format(e.to_string()))?;
            let record = match json.get("record") {
                Some(r) => Some(
                    serde_json::from_value::<RunRecord>(r.clone())
                        .map_err(|e| format(e.to_string()))?,
                ),
                None => None,
            };
            // Files written before run records existed hold only the result.
            let result = json.get("result").unwrap_or(&json);
            let run = LabRunResult::from_json(result).map_err(|e| format(e.to_string()))?;
            out.push(SavedRun {
                result: run,
                record,
            });
        }
        out.sort_by_key(|r| r.result.run_number);
        Ok(out)
    }

    /// Starts `table.csv` afresh with the given runs (the saved ones, when resuming), and
    /// returns the writer for the rest.
    pub fn start_table(
        &self,
        experiment: &LoadedExperiment,
        runs: &[LabRunResult],
    ) -> Result<TableWriter<BufWriter<File>>, ResultsError> {
        let path = self.dir.join(TABLE_FILE);
        let file = File::create(&path).map_err(io(&path))?;
        let model = self.meta.model.to_string_lossy();
        let mut w = TableWriter::new(
            BufWriter::new(file),
            &self.meta.netlogo_version,
            &model,
            experiment,
        )
        .map_err(io(&path))?;
        for r in runs {
            w.write_run(r).map_err(io(&path))?;
        }
        Ok(w)
    }

    /// Writes `results.parquet` from the saved runs.
    pub fn write_parquet(&self, experiment: &LoadedExperiment) -> Result<PathBuf, ResultsError> {
        let runs = self.saved_runs()?;
        let refs: Vec<&LabRunResult> = runs.iter().collect();
        let path = self.dir.join(PARQUET_FILE);
        let model = self.meta.model.to_string_lossy();
        write_parquet(
            &path,
            experiment,
            &refs,
            &FileInfo {
                model_path: &model,
                netlogo_version: &self.meta.netlogo_version,
            },
        )?;
        Ok(path)
    }
}

/// Writes a file through a temporary name and a rename, so it is never seen half written.
fn write_whole(path: &Path, bytes: &[u8]) -> Result<(), ResultsError> {
    let partial = path.with_extension("partial");
    std::fs::write(&partial, bytes).map_err(io(&partial))?;
    std::fs::rename(&partial, path).map_err(io(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use logos_bridge_client::protocol::WorldDimensions;
    use serde_json::json;

    fn experiment() -> LoadedExperiment {
        LoadedExperiment {
            id: 1,
            name: "E".into(),
            repetitions: 1,
            sequential_run_order: true,
            run_metrics_every_step: false,
            run_metrics_condition: String::new(),
            time_limit: 0,
            exit_condition: String::new(),
            pre_experiment: String::new(),
            setup: String::new(),
            go: String::new(),
            post_run: String::new(),
            post_experiment: String::new(),
            metrics: vec!["n".into()],
            variables: vec!["a".into()],
            runs: vec![Vec::new(), Vec::new()],
            world_dimensions: WorldDimensions {
                min_pxcor: 0,
                max_pxcor: 0,
                min_pycor: 0,
                max_pycor: 0,
            },
        }
    }

    fn meta() -> RunMeta {
        RunMeta {
            experiment: "E".into(),
            model: "/m/model.nlogox".into(),
            project: "/m".into(),
            setup_file: None,
            netlogo_version: "7.0.4".into(),
            run_count: 2,
            variables: vec!["a".into()],
            metrics: vec!["n".into()],
            update_plots: false,
            created: "2026-10-11T09:00:00-07:00".into(),
            provenance: None,
        }
    }

    fn run(n: u64, reason: &str) -> LabRunResult {
        LabRunResult::from_json(&json!({
            "runNumber": n,
            "settings": [{"name": "a", "value": 1.0, "text": "1"}],
            "reason": reason,
            "steps": 3,
            "measurements": [{"step": 3, "values": [{"value": 2.5, "text": "2.5"}]}],
            "errors": [],
            "elapsedMillis": 1.0
        }))
        .unwrap()
    }

    fn record(n: u64) -> RunRecord {
        RunRecord {
            run_number: n,
            started: "2026-10-11T09:00:00-07:00".into(),
            ended: "2026-10-11T09:00:01-07:00".into(),
            worker: 0,
            random_seed: None,
            provenance: Provenance {
                logos_version: "0.0.0".into(),
                netlogo_version: "7.0.4".into(),
                bridge_version: "0.0.0".into(),
                java_version: "17".into(),
                java_vendor: "x".into(),
                os: "linux".into(),
                arch: "x86_64".into(),
                files: Vec::new(),
                secrets: Vec::new(),
            },
        }
    }

    #[test]
    fn saves_runs_and_resumes() {
        let dir = std::env::temp_dir().join(format!("logos-results-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let r = ResultsDir::create(&dir, meta()).unwrap();
        assert!(r.save_run(&run(2, "completed"), &record(2)).unwrap());
        assert!(!r.save_run(&run(1, "halted"), &record(1)).unwrap());
        assert!(ResultsDir::create(&dir, meta()).is_err(), "folder in use");

        let r = ResultsDir::open(&dir).unwrap();
        assert_eq!(r.meta, meta());
        let saved = r.saved_runs().unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0], run(2, "completed"));
        r.meta.check(&experiment()).unwrap();
        let mut changed = experiment();
        changed.metrics.push("m".into());
        assert!(r.meta.check(&changed).is_err());

        let w = r.start_table(&experiment(), &saved).unwrap();
        drop(w);
        let table = std::fs::read_to_string(dir.join(TABLE_FILE)).unwrap();
        assert!(table.contains("\"2\",\"1\",\"3\",\"2.5\""));

        // A run file written before run records existed.
        std::fs::write(
            dir.join(RUNS_DIR).join("000001.json"),
            serde_json::to_vec(&run(1, "completed").raw).unwrap(),
        )
        .unwrap();
        let both = r.saved().unwrap();
        assert_eq!(both[0].record, None);
        assert_eq!(both[1].record, Some(record(2)));
        let p = r.write_parquet(&experiment()).unwrap();
        let back = crate::parquet::read_parquet(&p).unwrap();
        assert_eq!(back.rows.len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
