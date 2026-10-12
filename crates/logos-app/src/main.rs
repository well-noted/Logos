//! The Logos desktop app (Phase 2, `docs/phase-2-spec.md`). This is the Tauri host: it owns
//! the run bridge, the open models and the settings, and answers the front end in `app/`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod engine;
mod experiments;
mod git;
mod lsp;
mod project;
mod session;
mod settings;
mod values;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use logos_bridge_client::Bridge;
use logos_nlogox::WidgetValue;
use serde::Serialize;
use serde_json::{Value as Json, json};
use session::{AppError, AppResult, Emit, ModelView, OutlineEntry, Sessions};
use settings::{Settings, Store};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{Emitter, Manager, State};

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    /// `searching`, `notFound`, `starting`, `ready` or `failed`.
    pub state: String,
    pub installs: Vec<engine::Install>,
    pub home: Option<PathBuf>,
    pub version: Option<String>,
    /// `engine.hello`, when the bridge is ready.
    pub hello: Option<Json>,
    pub error: Option<String>,
    /// The bridge's last log lines, when it failed.
    pub log: Option<String>,
}

struct Core {
    store: Store,
    lsp: Mutex<Option<Arc<lsp::LspProxy>>>,
    /// The passphrase of the encrypted secrets file, when there is no credential store and the
    /// user typed it in this session. Kept in memory only.
    passphrase: Mutex<Option<String>>,
    studies: Arc<experiments::Studies>,
    /// The engine's bridge configuration, for the bridges experiments start.
    bridge_config: Mutex<Option<logos_bridge_client::BridgeConfig>>,
    settings: Mutex<Settings>,
    engine: Mutex<EngineStatus>,
    sessions: Mutex<Option<Arc<Sessions>>>,
    emit: Emit,
}

impl Core {
    fn sessions(&self) -> AppResult<Arc<Sessions>> {
        self.sessions.lock().unwrap().clone().ok_or_else(|| {
            AppError::new("NetLogo isn't running yet. Check the engine on the start screen.")
        })
    }

    fn set_engine(&self, status: EngineStatus) {
        (self.emit)("engine", serde_json::to_value(&status).unwrap_or_default());
        *self.engine.lock().unwrap() = status;
    }

    /// Finds NetLogo and starts the run bridge. Called at startup and when the user picks a
    /// different install.
    fn start_engine(&self) {
        if let Some(old) = self.sessions.lock().unwrap().take() {
            old.shutdown();
        }
        let saved = self.settings.lock().unwrap().netlogo_home.clone();
        let installs = engine::find_installs(saved.as_deref());
        let chosen = saved
            .as_ref()
            .and_then(|s| installs.iter().find(|i| &i.path == s))
            .filter(|i| i.version == engine::NETLOGO_VERSION)
            .or_else(|| engine::choose(&installs))
            .cloned();
        let Some(install) = chosen else {
            self.set_engine(EngineStatus {
                state: "notFound".into(),
                installs,
                ..EngineStatus::default()
            });
            return;
        };
        let base = EngineStatus {
            installs: installs.clone(),
            home: Some(install.path.clone()),
            version: Some(install.version.clone()),
            ..EngineStatus::default()
        };
        let Some(jar) = engine::bridge_jar() else {
            self.set_engine(EngineStatus {
                state: "failed".into(),
                error: Some(
                    "The bridge jar wasn't found. Build it with `cd bridge; .\\gradlew.bat jar`, \
                     or set LOGOS_BRIDGE_JAR."
                        .into(),
                ),
                ..base
            });
            return;
        };
        self.set_engine(EngineStatus {
            state: "starting".into(),
            ..base.clone()
        });
        let config = engine::run_config(&install.path, &install.version, jar, 0);
        *self.bridge_config.lock().unwrap() = Some(config.clone());
        match Bridge::spawn(&config) {
            Ok(bridge) => {
                let hello = serde_json::to_value(bridge.hello()).ok();
                let bridge = Arc::new(bridge);
                let sessions = Sessions::new(bridge, self.emit.clone());
                *self.sessions.lock().unwrap() = Some(sessions.clone());
                // The language server checks code in its own bridge (D-25), capped so that
                // both bridges fit beside each other.
                let mut check = engine::run_config(
                    &install.path,
                    &install.version,
                    engine::bridge_jar().unwrap_or_default(),
                    CHECK_BRIDGE_HEAP_MB,
                );
                check.java = config.java.clone();
                let emit = self.emit.clone();
                let s2 = sessions.clone();
                let apply: Arc<dyn Fn(lsp::OutsideEdits, String) -> bool + Send + Sync> =
                    Arc::new(move |edits: lsp::OutsideEdits, server_text: String| {
                        let Some(path) = edits.first().map(|e| e.0.clone()) else {
                            return false;
                        };
                        let edits = edits.into_iter().map(|(_, r, t)| (r, t)).collect();
                        match s2.apply_outside_edits(&path, &server_text, edits) {
                            Some((id, _)) => {
                                emit("model-widgets", json!({ "model": id }));
                                true
                            }
                            None => false,
                        }
                    });
                *self.lsp.lock().unwrap() =
                    Some(lsp::LspProxy::start(Some(check), self.emit.clone(), apply));
                self.set_engine(EngineStatus {
                    state: "ready".into(),
                    hello,
                    ..base
                });
            }
            Err(e) => {
                let log = match &e {
                    logos_bridge_client::ClientError::Closed { stderr } => Some(stderr.clone()),
                    _ => None,
                };
                self.set_engine(EngineStatus {
                    state: "failed".into(),
                    error: Some(e.to_string()),
                    log,
                    ..base
                });
            }
        }
    }

    fn save_settings(&self) {
        let s = self.settings.lock().unwrap().clone();
        let _ = self.store.save(&s);
    }
}

type CoreState<'a> = State<'a, Arc<Core>>;

/// Heap limit of the language server's check bridge (D-25). It compiles code and never runs
/// models; Phase 2 measurements may change this.
const CHECK_BRIDGE_HEAP_MB: u32 = 1024;

/// Runs blocking work off the main thread.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| AppError::new(e.to_string()))?
}

// ---------------------------------------------------------------------------- app and engine

#[tauri::command]
fn app_info(core: CoreState) -> Json {
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "dataDir": core.store.dir(),
        "platform": std::env::consts::OS,
        "netlogoVersion": engine::NETLOGO_VERSION,
    })
}

/// A line from the front end for the app's log (errors and warnings it can't show).
#[tauri::command]
fn ui_log(level: String, text: String) {
    eprintln!("ui {level}: {text}");
}

/// A model named on the command line (`logos-app <model>`), to open once NetLogo is ready.
#[tauri::command]
fn startup_model() -> Option<PathBuf> {
    std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .find(|p| {
            p.extension().is_some_and(|e| {
                e.eq_ignore_ascii_case("nlogox") || e.eq_ignore_ascii_case("nlogo")
            })
        })
        .and_then(|p| std::path::absolute(p).ok())
}

#[tauri::command]
fn engine_status(core: CoreState) -> EngineStatus {
    core.engine.lock().unwrap().clone()
}

#[tauri::command]
async fn engine_set_home(core: CoreState<'_>, path: PathBuf) -> AppResult<EngineStatus> {
    let core = core.inner().clone();
    blocking(move || {
        if engine::install_version(&path).is_none() {
            return Err(AppError::new(format!(
                "{} doesn't look like a NetLogo folder: it has no app\\netlogo-<version>.jar",
                path.display()
            )));
        }
        core.settings.lock().unwrap().netlogo_home = Some(path);
        core.save_settings();
        core.start_engine();
        Ok(core.engine.lock().unwrap().clone())
    })
    .await
}

#[tauri::command]
async fn engine_restart(core: CoreState<'_>) -> AppResult<EngineStatus> {
    let core = core.inner().clone();
    blocking(move || {
        core.start_engine();
        Ok(core.engine.lock().unwrap().clone())
    })
    .await
}

// -------------------------------------------------------------------------------- settings

#[tauri::command]
fn settings_get(core: CoreState) -> Settings {
    core.settings.lock().unwrap().clone()
}

#[tauri::command]
fn settings_set(core: CoreState, settings: Settings) {
    *core.settings.lock().unwrap() = settings;
    core.save_settings();
}

#[tauri::command]
fn themes_list(core: CoreState) -> Vec<Json> {
    core.store
        .custom_themes()
        .into_iter()
        .map(|(name, text)| json!({ "name": name, "json": text }))
        .collect()
}

#[tauri::command]
fn theme_save(core: CoreState, name: String, json: String) -> AppResult<()> {
    core.store
        .save_custom_theme(&name, &json)
        .map_err(AppError::new)
}

#[tauri::command]
fn theme_delete(core: CoreState, name: String) -> AppResult<()> {
    core.store.delete_custom_theme(&name).map_err(AppError::new)
}

/// Writes a theme the user exports to the file they chose in the save dialog.
#[tauri::command]
fn theme_export(path: PathBuf, json: String) -> AppResult<()> {
    serde_json::from_str::<Json>(&json).map_err(|e| AppError::new(format!("not a theme: {e}")))?;
    std::fs::write(&path, json).map_err(|e| AppError::new(format!("{}: {e}", path.display())))
}

/// Reads a theme file the user chose to import. Only small JSON files are accepted.
#[tauri::command]
fn theme_import(path: PathBuf) -> AppResult<String> {
    let meta =
        std::fs::metadata(&path).map_err(|e| AppError::new(format!("{}: {e}", path.display())))?;
    if meta.len() > 256 * 1024 {
        return Err(AppError::new("That file is too large to be a theme."));
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| AppError::new(format!("{}: {e}", path.display())))?;
    serde_json::from_str::<Json>(&text).map_err(|e| AppError::new(format!("not a theme: {e}")))?;
    Ok(text)
}

// ---------------------------------------------------------------------------------- models

#[tauri::command]
async fn model_open(core: CoreState<'_>, path: PathBuf) -> AppResult<ModelView> {
    let core = core.inner().clone();
    blocking(move || {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if ext == "nlogo" || ext == "nlogo3d" {
            return Err(AppError {
                message: "legacy".into(),
                span: None,
                detail: Some(suggest_converted(&path).display().to_string()),
            });
        }
        let view = core.sessions()?.open(&path)?;
        core.settings.lock().unwrap().add_recent(&path);
        core.save_settings();
        Ok(view)
    })
    .await
}

/// Where a converted copy of a legacy model goes: next to it, with `.nlogox`, and a number
/// when that name is taken. The original is never changed (D-06).
fn suggest_converted(source: &Path) -> PathBuf {
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "model".into());
    let dir = source.parent().unwrap_or(Path::new("."));
    let first = dir.join(format!("{stem}.nlogox"));
    if !first.exists() {
        return first;
    }
    (2..1000)
        .map(|n| dir.join(format!("{stem} ({n}).nlogox")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

#[tauri::command]
async fn model_convert(core: CoreState<'_>, source: PathBuf, output: PathBuf) -> AppResult<Json> {
    let core = core.inner().clone();
    blocking(move || {
        let r = core.sessions()?.bridge().convert_legacy(&source, &output)?;
        Ok(serde_json::to_value(&r).unwrap_or_default())
    })
    .await
}

#[tauri::command]
async fn model_attach_view(core: CoreState<'_>, id: u64, channel: Channel) -> AppResult<()> {
    let core = core.inner().clone();
    blocking(move || {
        let sink: session::FrameSink = Arc::new(move |bytes: Vec<u8>| {
            let _ = channel.send(InvokeResponseBody::Raw(bytes));
        });
        core.sessions()?.attach_view(id, sink)
    })
    .await
}

#[tauri::command]
async fn model_close(core: CoreState<'_>, id: u64) -> AppResult<()> {
    let core = core.inner().clone();
    blocking(move || core.sessions()?.close(id)).await
}

#[tauri::command]
async fn model_command(
    core: CoreState<'_>,
    id: u64,
    source: String,
    agents: String,
) -> AppResult<()> {
    let core = core.inner().clone();
    blocking(move || {
        core.sessions()?
            .command(id, &session::for_agents(&agents, &source))
    })
    .await
}

#[tauri::command]
async fn model_press(core: CoreState<'_>, id: u64, index: usize) -> AppResult<()> {
    let core = core.inner().clone();
    blocking(move || core.sessions()?.press_button(id, index)).await
}

#[tauri::command]
async fn model_step(core: CoreState<'_>, id: u64) -> AppResult<()> {
    let core = core.inner().clone();
    blocking(move || core.sessions()?.step(id)).await
}

#[tauri::command]
async fn model_stop(core: CoreState<'_>, id: u64) -> AppResult<()> {
    let core = core.inner().clone();
    blocking(move || core.sessions()?.stop(id)).await
}

#[tauri::command]
async fn model_set_value(
    core: CoreState<'_>,
    id: u64,
    index: usize,
    value: WidgetValue,
) -> AppResult<()> {
    let core = core.inner().clone();
    blocking(move || core.sessions()?.set_value(id, index, &value)).await
}

#[tauri::command]
async fn model_speed(core: CoreState<'_>, id: u64, speed: u64) -> AppResult<()> {
    let core = core.inner().clone();
    blocking(move || core.sessions()?.set_speed(id, speed)).await
}

#[tauri::command]
async fn model_view_updates(core: CoreState<'_>, id: u64, on_ticks: bool) -> AppResult<()> {
    let core = core.inner().clone();
    blocking(move || core.sessions()?.set_view_updates(id, on_ticks)).await
}

#[tauri::command]
async fn model_observe(core: CoreState<'_>, id: u64, reporters: Vec<String>) -> AppResult<Json> {
    let core = core.inner().clone();
    blocking(move || core.sessions()?.observe(id, &reporters)).await
}

#[tauri::command]
async fn model_set_code(core: CoreState<'_>, id: u64, code: String) -> AppResult<Json> {
    let core = core.inner().clone();
    blocking(move || {
        let s = core.sessions()?;
        let dirty = s.set_code(id, &code)?;
        let changed = s.get(id)?.code_changed_since_apply();
        Ok(json!({ "dirty": dirty, "notApplied": changed }))
    })
    .await
}

#[tauri::command]
async fn model_set_info(core: CoreState<'_>, id: u64, info: String) -> AppResult<bool> {
    let core = core.inner().clone();
    blocking(move || core.sessions()?.set_info(id, &info)).await
}

#[tauri::command]
async fn model_save(core: CoreState<'_>, id: u64) -> AppResult<()> {
    let core = core.inner().clone();
    blocking(move || core.sessions()?.save(id)).await
}

#[tauri::command]
async fn model_apply(core: CoreState<'_>, id: u64) -> AppResult<ModelView> {
    let core = core.inner().clone();
    blocking(move || core.sessions()?.apply(id)).await
}

#[tauri::command]
async fn model_outline(core: CoreState<'_>, id: u64) -> AppResult<Vec<OutlineEntry>> {
    let core = core.inner().clone();
    blocking(move || core.sessions()?.outline(id)).await
}

// ---------------------------------------------------------------------------- language server

#[tauri::command]
fn lsp_send(core: CoreState, message: String) -> AppResult<()> {
    let proxy = core.lsp.lock().unwrap().clone();
    let Some(proxy) = proxy else {
        return Err(AppError::new("the language server isn't running"));
    };
    let sessions = core.sessions.lock().unwrap().clone();
    proxy.editor_message(&message, |path| sessions.as_ref()?.text_of(path));
    Ok(())
}

#[tauri::command]
fn lsp_code_uri(core: CoreState, id: u64) -> AppResult<String> {
    let s = core.sessions()?.get(id)?;
    lsp::LspProxy::code_uri(&s.path).ok_or_else(|| AppError::new("the model's path can't be a URI"))
}

#[tauri::command]
async fn model_widgets(core: CoreState<'_>, id: u64) -> AppResult<Vec<logos_nlogox::Widget>> {
    let core = core.inner().clone();
    blocking(move || {
        let s = core.sessions()?;
        let widgets = s.widgets_of(id)?;
        if let (Some(proxy), Some(m)) = (core.lsp.lock().unwrap().clone(), s.get(id).ok())
            && let Some(text) = s.text_of(&m.path)
        {
            proxy.refresh(&m.path, &text);
        }
        Ok(widgets)
    })
    .await
}

// ------------------------------------------------------------------------ interface editing

#[derive(serde::Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
enum WidgetOp {
    Bounds {
        index: usize,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
    Attr {
        index: usize,
        name: String,
        value: String,
    },
    Text {
        index: usize,
        text: String,
    },
    Add {
        kind: String,
        x: i32,
        y: i32,
        name: String,
    },
    Delete {
        index: usize,
    },
    World {
        index: usize,
        world: logos_nlogox::widgets::World,
    },
    Part {
        index: usize,
        part: String,
        text: String,
    },
    Batch {
        ops: Vec<WidgetOp>,
    },
}

fn apply_widget_op(n: &mut logos_nlogox::Nlogox, op: WidgetOp) -> Result<(), String> {
    let r = match op {
        WidgetOp::Bounds {
            index,
            x,
            y,
            width,
            height,
        } => n.set_widget_bounds(index, x, y, width.max(10), height.max(10)),
        WidgetOp::Attr { index, name, value } => n.set_widget_attribute(index, &name, &value),
        WidgetOp::Text { index, text } => n.set_widget_text(index, &text),
        WidgetOp::Part { index, part, text } => n.set_widget_part(index, &part, &text),
        WidgetOp::Add { kind, x, y, name } => {
            let xml = logos_nlogox::widgets::new_widget_xml(&kind, x, y, &name)
                .ok_or_else(|| format!("unknown widget kind {kind}"))?;
            n.add_widget(&xml).map(|_| ())
        }
        WidgetOp::Delete { index } => n.delete_widget(index),
        WidgetOp::World { index, world } => n.set_world(index, &world),
        WidgetOp::Batch { ops } => {
            for op in ops {
                apply_widget_op(n, op)?;
            }
            Ok(())
        }
    };
    r.map_err(|e| e.to_string())
}

/// One interface edit, or several as one undo step (`batch`, used by replace).
#[tauri::command]
async fn model_edit_widget(
    core: CoreState<'_>,
    id: u64,
    op: WidgetOp,
) -> AppResult<session::WidgetEdit> {
    let core = core.inner().clone();
    blocking(move || {
        core.sessions()?
            .edit_widgets(id, |n| apply_widget_op(n, op))
    })
    .await
}

#[tauri::command]
async fn git_status(core: CoreState<'_>, id: u64) -> AppResult<Option<git::GitStatus>> {
    let path = model_path(&core, id)?;
    blocking(move || Ok(path.parent().and_then(git::status))).await
}

#[tauri::command]
async fn model_undo(core: CoreState<'_>, id: u64, redo: bool) -> AppResult<session::WidgetEdit> {
    let core = core.inner().clone();
    blocking(move || core.sessions()?.undo_redo(id, redo)).await
}

// ------------------------------------------------------------------------- projects, secrets

fn model_path(core: &Core, id: u64) -> AppResult<PathBuf> {
    Ok(core.sessions()?.get(id)?.path.clone())
}

#[tauri::command]
async fn project_status(core: CoreState<'_>, id: u64) -> AppResult<project::ProjectView> {
    let core = core.inner().clone();
    blocking(move || project::status(&model_path(&core, id)?)).await
}

#[tauri::command]
async fn project_detect(
    core: CoreState<'_>,
    id: u64,
    code: String,
) -> AppResult<project::DetectionView> {
    let core = core.inner().clone();
    blocking(move || project::detect_for(&model_path(&core, id)?, &code)).await
}

#[tauri::command]
async fn project_init(
    core: CoreState<'_>,
    id: u64,
    choice: project::InitChoice,
) -> AppResult<project::ProjectView> {
    let core = core.inner().clone();
    blocking(move || project::init(&model_path(&core, id)?, &choice)).await
}

#[tauri::command]
async fn project_set_role(
    core: CoreState<'_>,
    id: u64,
    path: String,
    role: String,
) -> AppResult<project::ProjectView> {
    let core = core.inner().clone();
    blocking(move || project::set_file_role(&model_path(&core, id)?, &path, &role)).await
}

#[tauri::command]
async fn project_check(core: CoreState<'_>, id: u64) -> AppResult<Vec<project::FindingView>> {
    let core = core.inner().clone();
    blocking(move || {
        let pass = core.passphrase.lock().unwrap().clone();
        project::check_project(&model_path(&core, id)?, pass.as_deref())
    })
    .await
}

#[tauri::command]
async fn secrets_list(core: CoreState<'_>, id: u64) -> AppResult<project::SecretsView> {
    let core = core.inner().clone();
    blocking(move || {
        let pass = core.passphrase.lock().unwrap().clone();
        project::list(&model_path(&core, id)?, pass.as_deref())
    })
    .await
}

#[tauri::command]
async fn secrets_unlock(
    core: CoreState<'_>,
    id: u64,
    passphrase: String,
) -> AppResult<project::SecretsView> {
    let core = core.inner().clone();
    blocking(move || {
        let view = project::list(&model_path(&core, id)?, Some(&passphrase))?;
        *core.passphrase.lock().unwrap() = Some(passphrase);
        Ok(view)
    })
    .await
}

#[tauri::command]
async fn secret_set(
    core: CoreState<'_>,
    id: u64,
    name: String,
    value: String,
    scope: String,
) -> AppResult<()> {
    let core = core.inner().clone();
    blocking(move || {
        let pass = core.passphrase.lock().unwrap().clone();
        project::set(
            &model_path(&core, id)?,
            pass.as_deref(),
            &name,
            &value,
            &scope,
        )
    })
    .await
}

#[tauri::command]
async fn secret_delete(
    core: CoreState<'_>,
    id: u64,
    name: String,
    scope: String,
) -> AppResult<bool> {
    let core = core.inner().clone();
    blocking(move || {
        let pass = core.passphrase.lock().unwrap().clone();
        project::delete(&model_path(&core, id)?, pass.as_deref(), &name, &scope)
    })
    .await
}

/// Shows a value to the user, who confirmed it in the front end. Logged by name and time.
#[tauri::command]
async fn secret_unmask(core: CoreState<'_>, id: u64, name: String) -> AppResult<String> {
    let core = core.inner().clone();
    blocking(move || {
        let pass = core.passphrase.lock().unwrap().clone();
        project::reveal(&model_path(&core, id)?, pass.as_deref(), &name, "unmask")
    })
    .await
}

/// Copies a value to the clipboard and clears it after 30 seconds if it is still there.
#[tauri::command]
async fn secret_copy(
    app: tauri::AppHandle,
    core: CoreState<'_>,
    id: u64,
    name: String,
) -> AppResult<()> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    let core = core.inner().clone();
    let value = blocking(move || {
        let pass = core.passphrase.lock().unwrap().clone();
        project::reveal(&model_path(&core, id)?, pass.as_deref(), &name, "copy")
    })
    .await?;
    app.clipboard()
        .write_text(value.clone())
        .map_err(|e| AppError::new(e.to_string()))?;
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(30));
        if app.clipboard().read_text().is_ok_and(|t| t == value) {
            let _ = app.clipboard().write_text(String::new());
        }
    });
    Ok(())
}

#[tauri::command]
fn secrets_recent() -> Vec<String> {
    project::recent_access(8)
}

// ----------------------------------------------------------------------------- experiments

#[tauri::command]
async fn experiments_list(
    core: CoreState<'_>,
    id: u64,
) -> AppResult<Vec<experiments::ExperimentView>> {
    let core = core.inner().clone();
    blocking(move || {
        let s = core.sessions()?;
        let path = s.get(id)?.path.clone();
        let text = s.text_of(&path).unwrap_or_default();
        experiments::list(&path, &text)
    })
    .await
}

#[tauri::command]
async fn experiment_save_settings(
    core: CoreState<'_>,
    id: u64,
    name: String,
    settings: experiments::LogosSettings,
) -> AppResult<()> {
    let core = core.inner().clone();
    blocking(move || experiments::save_settings(&model_path(&core, id)?, &name, &settings)).await
}

#[tauri::command]
async fn experiment_run(
    core: CoreState<'_>,
    id: u64,
    name: String,
    settings: experiments::LogosSettings,
    resume: Option<PathBuf>,
) -> AppResult<u64> {
    let core = core.inner().clone();
    blocking(move || {
        let s = core.sessions()?;
        let session = s.get(id)?;
        if session.is_dirty() {
            return Err(AppError::new(
                "Save the model first: experiments run the model file as it is on disk.",
            ));
        }
        let config = core
            .bridge_config
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| AppError::new("NetLogo isn't running"))?;
        let req = experiments::StartRequest {
            model: session.path.clone(),
            experiment: name,
            settings,
            resume,
            config,
            passphrase: core.passphrase.lock().unwrap().clone(),
        };
        core.studies.start(req, core.emit.clone())
    })
    .await
}

#[tauri::command]
fn study_status(core: CoreState, id: u64) -> AppResult<experiments::StudyView> {
    core.studies.get(id)
}

#[tauri::command]
fn study_stop(core: CoreState, id: u64) -> AppResult<()> {
    core.studies.stop(id)
}

#[tauri::command]
async fn results_past(core: CoreState<'_>, id: u64) -> AppResult<Vec<experiments::PastRun>> {
    let core = core.inner().clone();
    blocking(move || Ok(experiments::past(&model_path(&core, id)?))).await
}

#[tauri::command]
async fn results_read(dir: PathBuf) -> AppResult<Json> {
    blocking(move || experiments::read_results(&dir, 2000)).await
}

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("logos"));
            let store = Store::new(dir);
            let settings = store.load();
            let handle = app.handle().clone();
            let emit: Emit = Arc::new(move |name: &str, payload: Json| {
                let _ = handle.emit(name, payload);
            });
            let core = Arc::new(Core {
                store,
                settings: Mutex::new(settings),
                engine: Mutex::new(EngineStatus {
                    state: "searching".into(),
                    ..EngineStatus::default()
                }),
                sessions: Mutex::new(None),
                lsp: Mutex::new(None),
                passphrase: Mutex::new(None),
                studies: Arc::new(experiments::Studies::default()),
                bridge_config: Mutex::new(None),
                emit,
            });
            app.manage(core.clone());
            std::thread::spawn(move || core.start_engine());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            ui_log,
            startup_model,
            engine_status,
            engine_set_home,
            engine_restart,
            settings_get,
            settings_set,
            themes_list,
            theme_save,
            theme_delete,
            theme_export,
            theme_import,
            model_open,
            model_convert,
            model_attach_view,
            model_close,
            model_command,
            model_press,
            model_step,
            model_stop,
            model_set_value,
            model_speed,
            model_view_updates,
            model_observe,
            model_set_code,
            model_set_info,
            model_save,
            model_apply,
            model_outline,
            model_widgets,
            model_edit_widget,
            model_undo,
            git_status,
            lsp_send,
            lsp_code_uri,
            project_status,
            project_detect,
            project_init,
            project_set_role,
            project_check,
            secrets_list,
            secrets_unlock,
            secret_set,
            secret_delete,
            secret_unmask,
            secret_copy,
            secrets_recent,
            experiments_list,
            experiment_save_settings,
            experiment_run,
            study_status,
            study_stop,
            results_past,
            results_read,
        ])
        .build(tauri::generate_context!())
        .expect("start Logos");
    let code = app.run_return(|handle, event| {
        if let tauri::RunEvent::Exit = event
            && let Some(core) = handle.try_state::<Arc<Core>>()
            && let Some(s) = core.sessions.lock().unwrap().take()
        {
            s.shutdown();
        }
    });
    // The bridge's stdin closes when its last reference goes; it exits within 2 s (A10).
    std::process::exit(code);
}
