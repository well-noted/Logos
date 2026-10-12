//! The Logos desktop app (Phase 2, `docs/phase-2-spec.md`). This is the Tauri host: it owns
//! the run bridge, the open models and the settings, and answers the front end in `app/`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod engine;
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
        match Bridge::spawn(&config) {
            Ok(bridge) => {
                let hello = serde_json::to_value(bridge.hello()).ok();
                let bridge = Arc::new(bridge);
                *self.sessions.lock().unwrap() = Some(Sessions::new(bridge, self.emit.clone()));
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

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
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
