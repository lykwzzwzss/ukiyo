use serde::Serialize;
use std::path::PathBuf;
use tauri::AppHandle;

mod autolibs;
mod fps;
mod hook;
mod ini;
mod legacy_mod_fixes;
mod migrations;
mod mods;
mod names;
mod prepare;
mod preload;
mod rules;
mod selection;
mod shared;
mod storage;
mod switching;
mod versioned;
mod work;

static WORKSPACE: once_cell::sync::Lazy<work::Coordinator> = once_cell::sync::Lazy::new(work::Coordinator::new);
static PREPARED: once_cell::sync::Lazy<std::sync::Mutex<preload::Cache<PrepareSummary>>> = once_cell::sync::Lazy::new(|| std::sync::Mutex::new(preload::Cache::new()));
#[cfg(test)]
static PREPARATION_RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[derive(Clone)]
struct ActiveWorkspace {
  root: PathBuf,
  game: String,
  mods: PathBuf,
  fps: u32,
}
static ACTIVE: once_cell::sync::Lazy<std::sync::Mutex<Option<ActiveWorkspace>>> = once_cell::sync::Lazy::new(|| std::sync::Mutex::new(None));
fn active_workspace() -> Option<ActiveWorkspace> { ACTIVE.lock().unwrap_or_else(|error| error.into_inner()).clone() }

async fn workspace<T: Send + 'static>(action: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
  tauri::async_runtime::spawn_blocking(move || {
    WORKSPACE.run(action)
  }).await.map_err(|error| error.to_string())?
}

pub use mods::{detect_version, scan_mods, ModEntry};
pub use prepare::PrepareSummary;

pub fn stop_hook() {
  fps::stop();
  if let Err(error) = hook::stop() { eprintln!("Failed to stop GIMI: {}", error); }
  *ACTIVE.lock().unwrap_or_else(|error| error.into_inner()) = None;
}

pub fn shutdown() {
  WORKSPACE.shutdown(|| {
    PREPARED.lock().unwrap_or_else(|error| error.into_inner()).clear();
    stop_hook();
  });
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GimiStatus {
  pub version: String,
  pub mods_path: String,
  pub enabled: bool,
  pub fps: u32,
  pub mods: Vec<ModEntry>,
  pub running: bool,
  pub pending_changes: usize,
}

fn config() -> crate::config::Configuration {
  crate::config::get_config()
}

fn env_root() -> Option<PathBuf> {
  std::env::var_os("UKIYO_GIMI_ROOT").map(PathBuf::from).filter(|path| path.join("3dmloader.dll").is_file())
}

pub fn cli_root() -> Result<PathBuf, String> {
  if let Some(root) = env_root() {
    return Ok(root);
  }
  let executable = std::env::current_exe().map_err(|error| error.to_string())?;
  let directory = executable.parent().ok_or_else(|| "无法定位程序目录".to_string())?;
  let candidate = directory.join("gimi");
  if candidate.join("3dmloader.dll").is_file() {
    Ok(candidate)
  } else {
    Err("找不到 GIMI 运行库（gimi/3dmloader.dll）".to_string())
  }
}

pub fn prepare_workspace(root: &std::path::Path, game: &std::path::Path, mods: &std::path::Path) -> Result<PrepareSummary, String> {
  #[cfg(test)]
  PREPARATION_RUNS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
  let mut cache = PREPARED.lock().unwrap_or_else(|error| error.into_inner());
  cache.clear();
  // Applying pending switches changes the source layout. Capture its stamp
  // inside the transaction, after switches are applied, not before.
  let mut before = None;
  let summary = selection::apply(root, mods, || {
    before = preload::input_stamp(root, game, mods).ok();
    prepare::prepare(root, game, mods)
  })?;
  if let Some(before) = before { cache.store(root, game, mods, &before, summary.clone()); }
  Ok(summary)
}

fn warm_workspace(root: &std::path::Path, game: &std::path::Path, mods: &std::path::Path) -> Result<bool, String> {
  if selection::has_pending(root, mods)? { return Ok(false); }
  ensure_prepared(root, game, mods)?;
  Ok(true)
}

fn ensure_prepared(root: &std::path::Path, game: &std::path::Path, mods: &std::path::Path) -> Result<PrepareSummary, String> {
  if !selection::has_pending(root, mods)? {
    if let Some(summary) = PREPARED.lock().unwrap_or_else(|error| error.into_inner()).get(root, game, mods) { return Ok(summary); }
  }
  prepare_workspace(root, game, mods)
}

#[tauri::command]
pub async fn gimi_preload(game: String) -> Result<bool, String> {
  workspace(move || {
    let config = config();
    if crate::game_lifecycle::is_running() || active_workspace().is_some()
      || !config.gimi_enabled.unwrap_or(false) || config.game_install_path.as_deref() != Some(game.as_str()) { return Ok(false); }
    let game = std::path::Path::new(&game);
    if !game.is_file() || detect_version(game).is_none() || fps::game_has_window(game) { return Ok(false); }
    let root = cli_root()?;
    let mods = config.gimi_mods_path.filter(|path| !path.trim().is_empty()).map(PathBuf::from).unwrap_or_else(|| root.join("Mods"));
    // Background work must never commit staged switches or rewrite a live game.
    warm_workspace(&root, game, &mods)
  }).await
}

pub fn runtime_root(app: &AppHandle) -> Result<PathBuf, String> {
  if let Some(active) = active_workspace() { return Ok(active.root); }
  if let Some(root) = env_root() {
    return Ok(root);
  }
  if let Ok(executable) = std::env::current_exe() {
    if let Some(directory) = executable.parent() {
      let candidate = directory.join("gimi");
      if candidate.join("3dmloader.dll").is_file() {
        return Ok(candidate);
      }
    }
  }
  let resource = app.path_resolver().resource_dir().ok_or_else(|| "无法定位程序目录".to_string())?;
  let candidate = resource.join("assets").join("gimi");
  if candidate.join("3dmloader.dll").is_file() {
    return Ok(candidate);
  }
  Err("找不到 GIMI 运行库（gimi/3dmloader.dll）".to_string())
}

pub fn mods_dir(app: &AppHandle) -> Result<PathBuf, String> {
  if let Some(active) = active_workspace() { return Ok(active.mods); }
  if let Some(path) = config().gimi_mods_path {
    if !path.trim().is_empty() {
      return Ok(PathBuf::from(path));
    }
  }
  Ok(runtime_root(app)?.join("Mods"))
}

#[tauri::command]
pub async fn gimi_status(app: AppHandle) -> Result<GimiStatus, String> {
  workspace(move || status(&app)).await
}

fn status(app: &AppHandle) -> Result<GimiStatus, String> {
  let config = config();
  let root = runtime_root(&app)?;
  let mods_path = mods_dir(&app)?;
  std::fs::create_dir_all(&mods_path).map_err(|error| format!("无法创建 Mods 目录：{}", error))?;
  let active = active_workspace();
  let game = active.as_ref().map(|active| active.game.clone()).or(config.game_install_path);
  let version = game
    .as_deref()
    .and_then(|path| detect_version(std::path::Path::new(path)))
    .unwrap_or_default();
  let mut mods = scan_mods(&mods_path)?;
  let pending_changes = selection::overlay(&root, &mods_path, &mut mods)?;
  names::overlay(&root, &mods_path, &mut mods)?;
  if let Ok(raw) = std::fs::read_to_string(root.join("User/hash-variants-report.json")) {
    if let Ok(report) = serde_json::from_str::<serde_json::Value>(&raw) {
      if report["target"].as_str() == Some(version.as_str()) {
        for item in &mut mods { item.compatibility = report["groups"][&item.name].as_str().unwrap_or("").to_string(); }
      }
    }
  }
  Ok(GimiStatus {
    version,
    mods_path: mods_path.to_string_lossy().into_owned(),
    enabled: active.is_some() || config.gimi_enabled.unwrap_or(false),
    fps: active.map(|active| active.fps).unwrap_or_else(|| config.gimi_fps.unwrap_or(0)),
    mods,
    running: crate::game_lifecycle::is_running(),
    pending_changes,
  })
}

#[tauri::command]
pub async fn gimi_refresh(app: AppHandle) -> Result<GimiStatus, String> {
  workspace(move || status(&app)).await
}

#[tauri::command]
pub async fn gimi_reload(app: AppHandle) -> Result<GimiStatus, String> {
  workspace(move || {
    prepare_for_app(&app)?;
    status(&app)
  }).await
}

#[tauri::command]
pub async fn gimi_toggle_mod(app: AppHandle, folder: String, enabled: bool) -> Result<usize, String> {
  workspace(move || {
    let root = runtime_root(&app)?;
    let mods = mods_dir(&app)?;
    selection::stage(&root, &mods, &folder, enabled)
  }).await
}

#[tauri::command]
pub fn gimi_fps_status() -> String {
  fps::state()
}

#[tauri::command]
pub async fn gimi_rename_mod(app: AppHandle, folder: String, name: String) -> Result<String, String> {
  workspace(move || names::rename(&runtime_root(&app)?, &mods_dir(&app)?, &folder, &name)).await
}

fn prepare_for_app(app: &AppHandle) -> Result<PrepareSummary, String> {
  let config = config();
  let game = game_for_preparation()
    .ok_or_else(|| "请先在设置里选择游戏安装路径。".to_string())?;
  let root = runtime_root(&app)?;
  let mods = mods_dir(&app)?;
  if config.gimi_mods_path.as_ref().map_or(true, |path| path.trim().is_empty()) {
    std::fs::create_dir_all(&mods).map_err(|error| error.to_string())?;
  }
  prepare_workspace(&root, std::path::Path::new(&game), &mods)
}

fn game_for_preparation() -> Option<String> {
  if let Some(active) = active_workspace() { return Some(active.game); }
  config().game_install_path.filter(|path| {
    let path = std::path::Path::new(path);
    path.is_file() && detect_version(path).is_some()
  })
}

// Called after the game launch reservation and network preflight succeed. Keep
// setup and teardown in Rust so a closed/reloaded WebView cannot leave hooks.
pub async fn start_for_game(game: String) -> Result<(), String> {
  workspace(move || {
    let config = config();
    if !config.gimi_enabled.unwrap_or(false) { return Ok(()); }
    if fps::game_has_window(std::path::Path::new(&game)) { return Err("请先退出已运行的游戏，再启用模型".into()); }
    let root = cli_root()?;
    let mods = config.gimi_mods_path.filter(|path| !path.trim().is_empty()).map(PathBuf::from).unwrap_or_else(|| root.join("Mods"));
    if !mods.exists() && mods == root.join("Mods") { std::fs::create_dir_all(&mods).map_err(|error| error.to_string())?; }
    let summary = ensure_prepared(&root, std::path::Path::new(&game), &mods)?;
    if summary.blocked_files > 0 { eprintln!("GIMI skipped {} incompatible INIs", summary.blocked_files); }
    hook::start(&root)?;
    if let Err(error) = fps::start(&root, std::path::Path::new(&game), config.gimi_fps.unwrap_or(0)) {
      let _ = hook::stop();
      return Err(error);
    }
    *ACTIVE.lock().unwrap_or_else(|error| error.into_inner()) = Some(ActiveWorkspace { root, game, mods, fps: config.gimi_fps.unwrap_or(0) });
    Ok(())
  }).await
}

#[tauri::command]
pub fn gimi_open_mods(app: AppHandle) -> Result<(), String> {
  let directory = mods_dir(&app)?;
  std::fs::create_dir_all(&directory).map_err(|error| format!("无法创建 Mods 目录：{}", error))?;
  crate::system_helpers::open_in_browser(directory.to_string_lossy().into_owned());
  Ok(())
}
