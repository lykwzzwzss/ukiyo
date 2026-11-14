use once_cell::sync::Lazy;
use std::sync::{atomic::{AtomicBool, Ordering}, Mutex};
use tauri::Manager;

static RUNNING: AtomicBool = AtomicBool::new(false);
static STARTED: AtomicBool = AtomicBool::new(false);
static APP: Lazy<Mutex<Option<tauri::AppHandle>>> = Lazy::new(|| Mutex::new(None));

pub fn initialize(app: tauri::AppHandle) {
  *APP.lock().unwrap_or_else(|e| e.into_inner()) = Some(app);
}

pub fn is_running() -> bool { RUNNING.load(Ordering::Acquire) }
pub fn is_started() -> bool { STARTED.load(Ordering::Acquire) }

pub fn reserve() -> Result<(), String> {
  RUNNING.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
    .map(|_| ()).map_err(|_| "游戏已经启动或正在启动".into())
}

fn emit(event: &str, running: bool) {
  if let Some(app) = APP.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
    if let Err(e) = app.emit_all(event, running) { eprintln!("Failed to emit {}: {}", event, e); }
  }
}

pub fn started() { STARTED.store(true, Ordering::Release); emit("game_state_changed", true); }

pub fn failed() {
  crate::gimi::stop_hook();
  STARTED.store(false, Ordering::Release);
  RUNNING.store(false, Ordering::Release);
  emit("game_state_changed", false);
}

pub fn finished() {
  crate::gimi::stop_hook();
  STARTED.store(false, Ordering::Release);
  crate::disconnect();
  // Keep the launch reservation until cleanup completes.
  crate::managed_patch::cleanup();
  RUNNING.store(false, Ordering::Release);
  emit("game_state_changed", false);
  emit("game_closed", false);
}
