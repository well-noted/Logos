//! The running bridge process and request/response handling.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, BufWriter};
use std::path::Path;
use std::process::{Child, ChildStdin, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use logos_protocol::experiment::{
    ExperimentCloseParams, ExperimentCommandParams, ExperimentLoadParams, ExperimentRunParams,
};
use logos_protocol::{
    CommandParams, ConvertLegacyParams, ConvertLegacyResult, DataFrame, HelloParams, HelloResult,
    Incoming, Kind, ObserveParams, ObserveResult, OpenParams, OpenResult, PROTOCOL_VERSION,
    PrimitivesParams, PrimitivesResult, ProgramResult, Request, RpcError, RunFinished, RunParams,
    RunStreams, WorkspaceId, WorkspaceParams, read_frame, write_frame,
};
use logos_protocol::{ExperimentCommandResult, LabRunResult, LoadedExperiment};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value as Json;

use crate::config::BridgeConfig;

/// How long to wait for a response when the caller doesn't say.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);
/// How long to wait for the process to exit after shutdown or closing stdin (A10 allows 2 s).
const EXIT_TIMEOUT: Duration = Duration::from_secs(3);
/// Lines of bridge stderr kept for error messages.
const STDERR_LINES: usize = 200;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("bridge configuration: {0}")]
    Config(String),
    #[error("cannot start the bridge: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("bridge returned an error: {0}")]
    Rpc(#[from] RpcError),
    #[error("no response to {method} within {timeout:?}")]
    Timeout { method: String, timeout: Duration },
    #[error("the bridge process has stopped. Last stderr lines:\n{stderr}")]
    Closed { stderr: String },
    #[error("cannot write to the bridge: {0}")]
    Write(#[source] logos_protocol::FrameError),
    #[error("unexpected response to {method}: {detail}")]
    BadResponse { method: String, detail: String },
}

/// A message the bridge sent without being asked (PROTOCOL.md section 5.5).
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub method: String,
    pub params: Json,
}

type Pending = Arc<Mutex<HashMap<u64, Sender<Result<Json, RpcError>>>>>;

/// A running bridge process. Dropping it closes stdin, which makes the bridge exit, and kills
/// the process if it hasn't exited within a few seconds.
pub struct Bridge {
    child: Mutex<Child>,
    stdin: Mutex<Option<BufWriter<ChildStdin>>>,
    pending: Pending,
    next_id: AtomicU64,
    notifications: Mutex<Receiver<Notification>>,
    /// Notifications taken off the channel while looking for a different one.
    held: Mutex<VecDeque<Notification>>,
    data_frames: Mutex<Receiver<DataFrame>>,
    stderr: Arc<Mutex<VecDeque<String>>>,
    threads: Vec<JoinHandle<()>>,
    hello: HelloResult,
}

impl Bridge {
    /// Starts the bridge and performs the `engine.hello` handshake.
    pub fn spawn(config: &BridgeConfig) -> Result<Bridge, ClientError> {
        config.check().map_err(ClientError::Config)?;
        let mut cmd = config
            .command()
            .map_err(|e| ClientError::Config(e.to_string()))?;
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(ClientError::Spawn)?;

        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr_pipe = child.stderr.take().expect("stderr is piped");

        let pending: Pending = Arc::default();
        let stderr: Arc<Mutex<VecDeque<String>>> = Arc::default();
        let (notify_tx, notify_rx) = mpsc::channel();
        let (data_tx, data_rx) = mpsc::channel();

        let reader = {
            let pending = Arc::clone(&pending);
            std::thread::Builder::new()
                .name("logos-bridge-reader".into())
                .spawn(move || read_loop(stdout, pending, notify_tx, data_tx))
                .expect("spawn reader thread")
        };
        let stderr_reader = {
            let stderr = Arc::clone(&stderr);
            std::thread::Builder::new()
                .name("logos-bridge-stderr".into())
                .spawn(move || {
                    for line in BufReader::new(stderr_pipe).lines() {
                        let Ok(line) = line else { break };
                        let mut buf = stderr.lock().unwrap();
                        if buf.len() == STDERR_LINES {
                            buf.pop_front();
                        }
                        buf.push_back(line);
                    }
                })
                .expect("spawn stderr thread")
        };

        let mut bridge = Bridge {
            child: Mutex::new(child),
            stdin: Mutex::new(Some(BufWriter::new(stdin))),
            pending,
            next_id: AtomicU64::new(1),
            notifications: Mutex::new(notify_rx),
            held: Mutex::new(VecDeque::new()),
            data_frames: Mutex::new(data_rx),
            stderr,
            threads: vec![reader, stderr_reader],
            hello: HelloResult::default(),
        };
        bridge.hello = bridge.call_typed(
            "engine.hello",
            HelloParams {
                protocol_version: PROTOCOL_VERSION,
                client: concat!("logos-bridge-client ", env!("CARGO_PKG_VERSION")),
            },
            Duration::from_secs(60),
        )?;
        Ok(bridge)
    }

    /// What the bridge reported in the handshake.
    pub fn hello(&self) -> &HelloResult {
        &self.hello
    }

    /// The operating system's process id of the bridge.
    pub fn pid(&self) -> u32 {
        self.child.lock().unwrap().id()
    }

    /// Sends a request and waits for its response.
    pub fn call(
        &self,
        method: &str,
        params: impl Serialize,
        timeout: Duration,
    ) -> Result<Json, ClientError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, tx);

        let payload = serde_json::to_vec(&Request::new(id, method, params))
            .expect("protocol requests always serialize");
        let written = {
            let mut stdin = self.stdin.lock().unwrap();
            match stdin.as_mut() {
                Some(w) => write_frame(w, Kind::Json, &payload),
                None => {
                    self.pending.lock().unwrap().remove(&id);
                    return Err(self.closed());
                }
            }
        };
        if let Err(e) = written {
            self.pending.lock().unwrap().remove(&id);
            // A broken pipe means the process has gone; report it that way.
            return Err(if self.has_exited() {
                self.closed()
            } else {
                ClientError::Write(e)
            });
        }

        match rx.recv_timeout(timeout) {
            Ok(result) => Ok(result?),
            Err(RecvTimeoutError::Timeout) => {
                self.pending.lock().unwrap().remove(&id);
                Err(ClientError::Timeout {
                    method: method.to_owned(),
                    timeout,
                })
            }
            Err(RecvTimeoutError::Disconnected) => Err(self.closed()),
        }
    }

    fn call_typed<T: DeserializeOwned>(
        &self,
        method: &str,
        params: impl Serialize,
        timeout: Duration,
    ) -> Result<T, ClientError> {
        let v = self.call(method, params, timeout)?;
        serde_json::from_value(v).map_err(|e| ClientError::BadResponse {
            method: method.to_owned(),
            detail: e.to_string(),
        })
    }

    // ------------------------------------------------------------ workspace methods

    /// Opens a model file in a new workspace. Relative paths are made absolute first, because
    /// the bridge runs in the NetLogo install folder.
    pub fn open(&self, path: impl AsRef<Path>) -> Result<OpenResult, ClientError> {
        let path = std::path::absolute(path.as_ref()).map_err(|e| {
            ClientError::Config(format!("cannot resolve {}: {e}", path.as_ref().display()))
        })?;
        let path = path.to_string_lossy();
        self.call_typed(
            "workspace.open",
            OpenParams { path: &path },
            DEFAULT_TIMEOUT,
        )
    }

    /// The compiled model's declarations and procedures as NetLogo holds them.
    pub fn program(&self, workspace: WorkspaceId) -> Result<ProgramResult, ClientError> {
        self.call_typed(
            "workspace.program",
            WorkspaceParams { workspace },
            DEFAULT_TIMEOUT,
        )
    }

    /// The syntax of every core primitive and of the given extensions' primitives (A8).
    pub fn primitives(&self, extensions: &[&str]) -> Result<PrimitivesResult, ClientError> {
        self.call_typed(
            "engine.primitives",
            PrimitivesParams { extensions },
            DEFAULT_TIMEOUT,
        )
    }

    /// Converts a `.nlogo` file to `.nlogox` the way NetLogo's GUI does (A9). Fails with
    /// `FileExists` rather than replace `output`; never writes to `source`.
    pub fn convert_legacy(
        &self,
        source: impl AsRef<Path>,
        output: impl AsRef<Path>,
    ) -> Result<ConvertLegacyResult, ClientError> {
        let absolute = |p: &Path| {
            std::path::absolute(p)
                .map_err(|e| ClientError::Config(format!("cannot resolve {}: {e}", p.display())))
        };
        let source = absolute(source.as_ref())?;
        let output = absolute(output.as_ref())?;
        self.call_typed(
            "model.convertLegacy",
            ConvertLegacyParams {
                source_path: &source.to_string_lossy(),
                output_path: &output.to_string_lossy(),
            },
            DEFAULT_TIMEOUT,
        )
    }

    /// Opens a model from `.nlogox` text. Only for models that don't read files.
    pub fn open_string(&self, contents: &str) -> Result<OpenResult, ClientError> {
        #[derive(Serialize)]
        struct P<'a> {
            contents: &'a str,
            format: &'a str,
        }
        self.call_typed(
            "workspace.openString",
            P {
                contents,
                format: "nlogox",
            },
            DEFAULT_TIMEOUT,
        )
    }

    /// Runs commands as the observer.
    pub fn command(&self, workspace: WorkspaceId, source: &str) -> Result<(), ClientError> {
        self.command_with_timeout(workspace, source, DEFAULT_TIMEOUT)
    }

    pub fn command_with_timeout(
        &self,
        workspace: WorkspaceId,
        source: &str,
        timeout: Duration,
    ) -> Result<(), ClientError> {
        self.call(
            "workspace.command",
            CommandParams { workspace, source },
            timeout,
        )?;
        Ok(())
    }

    /// Evaluates reporters without affecting the run (D-14).
    pub fn observe(
        &self,
        workspace: WorkspaceId,
        reporters: &[&str],
    ) -> Result<ObserveResult, ClientError> {
        let v = self.call(
            "workspace.observe",
            ObserveParams {
                workspace,
                reporters,
            },
            DEFAULT_TIMEOUT,
        )?;
        ObserveResult::from_json(&v).map_err(|e| ClientError::BadResponse {
            method: "workspace.observe".into(),
            detail: e.to_string(),
        })
    }

    // ------------------------------------------------------------ experiment methods

    /// Loads a BehaviorSpace experiment from the workspace's model, or from a separate
    /// experiments file, and compiles it in the workspace (PROTOCOL.md section 5.4).
    pub fn experiment_load(
        &self,
        workspace: WorkspaceId,
        name: &str,
        file: Option<&Path>,
    ) -> Result<LoadedExperiment, ClientError> {
        let file = file
            .map(|f| {
                std::path::absolute(f).map_err(|e| {
                    ClientError::Config(format!("cannot resolve {}: {e}", f.display()))
                })
            })
            .transpose()?;
        let file = file.as_ref().map(|f| f.to_string_lossy());
        let v = self.call(
            "experiment.load",
            ExperimentLoadParams {
                workspace,
                name,
                file: file.as_deref(),
            },
            DEFAULT_TIMEOUT,
        )?;
        LoadedExperiment::from_json(&v).map_err(|e| ClientError::BadResponse {
            method: "experiment.load".into(),
            detail: e.to_string(),
        })
    }

    /// Runs one run of a loaded experiment and waits up to `timeout` for it to end.
    pub fn experiment_run(
        &self,
        workspace: WorkspaceId,
        experiment: u64,
        run_number: u64,
        update_plots: bool,
        timeout: Duration,
    ) -> Result<LabRunResult, ClientError> {
        let v = self.call(
            "experiment.run",
            ExperimentRunParams {
                workspace,
                experiment,
                run_number,
                update_plots,
            },
            timeout,
        )?;
        LabRunResult::from_json(&v).map_err(|e| ClientError::BadResponse {
            method: "experiment.run".into(),
            detail: e.to_string(),
        })
    }

    /// Runs the experiment's `preExperiment` or `postExperiment` commands.
    pub fn experiment_command(
        &self,
        workspace: WorkspaceId,
        experiment: u64,
        which: &str,
    ) -> Result<ExperimentCommandResult, ClientError> {
        self.call_typed(
            "experiment.command",
            ExperimentCommandParams {
                workspace,
                experiment,
                which,
            },
            DEFAULT_TIMEOUT,
        )
    }

    pub fn experiment_close(&self, experiment: u64) -> Result<(), ClientError> {
        self.call(
            "experiment.close",
            ExperimentCloseParams { experiment },
            DEFAULT_TIMEOUT,
        )?;
        Ok(())
    }

    pub fn dispose(&self, workspace: WorkspaceId) -> Result<(), ClientError> {
        self.call(
            "workspace.dispose",
            WorkspaceParams { workspace },
            DEFAULT_TIMEOUT,
        )?;
        Ok(())
    }

    /// Starts running `source` repeatedly (PROTOCOL.md, workspace.run). Returns the run id.
    /// Progress arrives as notifications and data frames; the run ends with `run.finished`.
    pub fn run(
        &self,
        workspace: WorkspaceId,
        source: &str,
        repeat: Option<u64>,
        stream: &RunStreams,
    ) -> Result<u64, ClientError> {
        let v = self.call(
            "workspace.run",
            RunParams {
                workspace,
                source,
                repeat,
                stream,
            },
            DEFAULT_TIMEOUT,
        )?;
        v.get("run")
            .and_then(Json::as_u64)
            .ok_or_else(|| ClientError::BadResponse {
                method: "workspace.run".into(),
                detail: v.to_string(),
            })
    }

    /// Asks a running workspace to stop after its current iteration.
    pub fn stop(&self, workspace: WorkspaceId) -> Result<(), ClientError> {
        self.call(
            "workspace.stop",
            WorkspaceParams { workspace },
            DEFAULT_TIMEOUT,
        )?;
        Ok(())
    }

    /// Takes the next notification, if one has arrived.
    pub fn try_notification(&self) -> Option<Notification> {
        if let Some(n) = self.held.lock().unwrap().pop_front() {
            return Some(n);
        }
        self.notifications.lock().unwrap().try_recv().ok()
    }

    /// Waits up to `timeout` for the next notification.
    pub fn next_notification(&self, timeout: Duration) -> Option<Notification> {
        if let Some(n) = self.held.lock().unwrap().pop_front() {
            return Some(n);
        }
        self.notifications
            .lock()
            .unwrap()
            .recv_timeout(timeout)
            .ok()
    }

    /// Waits for `run.finished` for this run. Other notifications are kept for
    /// `try_notification` and `next_notification`.
    pub fn wait_run_finished(
        &self,
        workspace: WorkspaceId,
        run: u64,
        timeout: Duration,
    ) -> Result<RunFinished, ClientError> {
        let matches = |n: &Notification| {
            n.method == "run.finished"
                && n.params.get("workspace").and_then(Json::as_u64) == Some(workspace.0)
                && n.params.get("run").and_then(Json::as_u64) == Some(run)
        };
        let parse = |n: Notification| {
            serde_json::from_value::<RunFinished>(n.params).map_err(|e| ClientError::BadResponse {
                method: "run.finished".into(),
                detail: e.to_string(),
            })
        };
        // Several threads may wait at once, each for its own run. The channel is read in short
        // slices so that a notification one thread takes off the channel for another thread
        // (and puts in `held`) is seen by that thread on its next pass.
        const SLICE: Duration = Duration::from_millis(20);
        let deadline = Instant::now() + timeout;
        loop {
            {
                let mut held = self.held.lock().unwrap();
                if let Some(i) = held.iter().position(matches) {
                    return parse(held.remove(i).unwrap());
                }
            }
            let left = deadline.saturating_duration_since(Instant::now());
            let received = self
                .notifications
                .lock()
                .unwrap()
                .recv_timeout(left.min(SLICE));
            match received {
                Ok(n) if matches(&n) => return parse(n),
                Ok(n) => self.held.lock().unwrap().push_back(n),
                Err(RecvTimeoutError::Timeout) if left.is_zero() => {
                    return Err(ClientError::Timeout {
                        method: "run.finished".into(),
                        timeout,
                    });
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Err(self.closed()),
            }
        }
    }

    /// Takes the next binary data frame (for example a view frame), if one has arrived.
    pub fn try_data_frame(&self) -> Option<DataFrame> {
        self.data_frames.lock().unwrap().try_recv().ok()
    }

    /// Waits up to `timeout` for the next binary data frame.
    pub fn next_data_frame(&self, timeout: Duration) -> Option<DataFrame> {
        self.data_frames.lock().unwrap().recv_timeout(timeout).ok()
    }

    // ------------------------------------------------------------ ending

    /// Asks the bridge to shut down and waits for the process to exit.
    pub fn shutdown(self) -> Result<ExitStatus, ClientError> {
        let sent = self.call("engine.shutdown", serde_json::json!({}), EXIT_TIMEOUT);
        let status = self.wait_or_kill(EXIT_TIMEOUT);
        sent?;
        status.ok_or_else(|| self.closed())
    }

    /// Closes the bridge's stdin, as happens when this process dies, and waits up to `timeout`
    /// for the bridge to exit. Returns None if it is still running (it is then killed on drop).
    pub fn close_stdin_and_wait(&self, timeout: Duration) -> Option<ExitStatus> {
        self.stdin.lock().unwrap().take();
        self.wait(timeout)
    }

    fn wait(&self, timeout: Duration) -> Option<ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(Some(status)) = self.child.lock().unwrap().try_wait() {
                return Some(status);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_or_kill(&self, timeout: Duration) -> Option<ExitStatus> {
        self.stdin.lock().unwrap().take();
        if let Some(status) = self.wait(timeout) {
            return Some(status);
        }
        let mut child = self.child.lock().unwrap();
        let _ = child.kill();
        child.wait().ok()
    }

    fn has_exited(&self) -> bool {
        matches!(self.child.lock().unwrap().try_wait(), Ok(Some(_)))
    }

    /// The last lines the bridge wrote to stderr.
    pub fn stderr_tail(&self) -> String {
        let buf = self.stderr.lock().unwrap();
        buf.iter().cloned().collect::<Vec<_>>().join("\n")
    }

    fn closed(&self) -> ClientError {
        // Give the stderr thread a moment to collect the last lines.
        std::thread::sleep(Duration::from_millis(50));
        ClientError::Closed {
            stderr: self.stderr_tail(),
        }
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.wait_or_kill(EXIT_TIMEOUT);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

/// Reads frames from the bridge's stdout and routes them until the stream ends.
fn read_loop(
    stdout: std::process::ChildStdout,
    pending: Pending,
    notify: Sender<Notification>,
    data: Sender<DataFrame>,
) {
    let mut reader = BufReader::with_capacity(1 << 16, stdout);
    // Ends on a clean end of stream or on a broken one; either way the process is gone.
    while let Ok(Some(frame)) = read_frame(&mut reader) {
        if frame.kind == Kind::Binary {
            // A frame that doesn't decode is a bridge bug; skip it rather than stop reading.
            if let Ok(f) = DataFrame::decode(&frame.payload) {
                let _ = data.send(f);
            }
            continue;
        }
        match Incoming::parse(&frame.payload) {
            Ok(Incoming::Response {
                id: Some(id),
                result,
            }) => {
                if let Some(tx) = pending.lock().unwrap().remove(&id) {
                    let _ = tx.send(result);
                }
            }
            Ok(Incoming::Response { id: None, .. }) | Err(_) => {
                // A response to a request the bridge couldn't read. The matching call times
                // out; nothing else can be done with it.
            }
            Ok(Incoming::Notification { method, params }) => {
                let _ = notify.send(Notification { method, params });
            }
        }
    }
    // Dropping the senders wakes every waiting call with "disconnected".
    pending.lock().unwrap().clear();
}
