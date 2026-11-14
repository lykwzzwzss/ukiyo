use once_cell::sync::Lazy;
use std::ffi::c_void;
use std::path::Path;
use std::sync::Mutex;


#[link(name = "kernel32")]
extern "system" {
  fn LoadLibraryW(path: *const u16) -> *mut c_void;
  fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
  fn FreeLibrary(module: *mut c_void) -> i32;
  fn CloseHandle(handle: *mut c_void) -> i32;
  fn GetModuleHandleW(path: *const u16) -> *mut c_void;
  fn GetLastError() -> u32;
}

#[link(name = "user32")]
extern "system" {
  fn UnhookWindowsHookEx(hook: *mut c_void) -> i32;
}

type HookLibrary = unsafe extern "C" fn(*const u16, *mut *mut c_void, *mut *mut c_void) -> i32;

struct HookState {
  module: *mut c_void,
  core: *mut c_void,
  hook: *mut c_void,
  mutex: *mut c_void,
}

unsafe impl Send for HookState {}

static STATE: Lazy<Mutex<Option<HookState>>> = Lazy::new(|| Mutex::new(None));

fn wide(path: &Path) -> Vec<u16> {
  use std::os::windows::ffi::OsStrExt;
  path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

pub fn stop() -> Result<(), String> {
  let mut guard = STATE.lock().map_err(|_| "挂钩状态已损坏".to_string())?;
  stop_state(&mut guard)
}

fn stop_state(slot: &mut Option<HookState>) -> Result<(), String> {
  let Some(state) = slot.as_mut() else {
    return Ok(());
  };
  unsafe {
    // The bundled UnhookLibrary closes the mutex first, then short-circuits
    // on a stale hook. Repeating it would close the same handle twice. Own
    // teardown here instead, and treat an already removed hook as success.
    if !state.hook.is_null() && UnhookWindowsHookEx(state.hook) == 0 {
      let error = GetLastError();
      if error != 1404 { // ERROR_INVALID_HOOK_HANDLE
        return Err(format!("清理加载钩子失败（Windows {}），请退出启动器后重试", error));
      }
    }
    if !state.mutex.is_null() { CloseHandle(state.mutex); }
    if !state.core.is_null() { FreeLibrary(state.core); }
    if !state.module.is_null() { FreeLibrary(state.module); }
  }
  *slot = None;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[link(name = "user32")]
  extern "system" {
    fn SetWindowsHookExW(kind: i32, callback: *mut c_void, module: *mut c_void, thread: u32) -> *mut c_void;
    fn CallNextHookEx(hook: *mut c_void, code: i32, wparam: usize, lparam: isize) -> isize;
  }
  #[link(name = "kernel32")]
  extern "system" {
    fn GetCurrentThreadId() -> u32;
    fn CreateMutexW(attributes: *mut c_void, owner: i32, name: *const u16) -> *mut c_void;
    fn GetHandleInformation(handle: *mut c_void, flags: *mut u32) -> i32;
  }
  unsafe extern "system" fn callback(code: i32, wparam: usize, lparam: isize) -> isize {
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
  }

  fn exercise_cleanup(already_removed: bool) {
    unsafe {
      // Thread-local test hook only: never attach to a game or other process.
      let hook = SetWindowsHookExW(5, callback as *mut c_void, std::ptr::null_mut(), GetCurrentThreadId());
      assert!(!hook.is_null());
      let mutex = CreateMutexW(std::ptr::null_mut(), 0, std::ptr::null());
      assert!(!mutex.is_null());
      if already_removed { assert_ne!(UnhookWindowsHookEx(hook), 0); }
      let mut state = Some(HookState { module: std::ptr::null_mut(), core: std::ptr::null_mut(), hook, mutex });
      stop_state(&mut state).unwrap();
      assert!(state.is_none(), "stale session must not block the next launch");
      let mut flags = 0;
      assert_eq!(GetHandleInformation(mutex, &mut flags), 0, "mutex must be released");
      let unrelated = CreateMutexW(std::ptr::null_mut(), 0, std::ptr::null());
      assert!(!unrelated.is_null());
      stop_state(&mut state).unwrap();
      assert_ne!(GetHandleInformation(unrelated, &mut flags), 0, "repeated cleanup must not close reused handles");
      CloseHandle(unrelated);
    }
  }

  #[test]
  fn cleans_active_hook_and_can_repeat() { exercise_cleanup(false); }

  #[test]
  fn already_removed_hook_releases_session_and_mutex() { exercise_cleanup(true); }
}

pub fn start(root: &Path) -> Result<(), String> {
  let mut guard = STATE.lock().map_err(|_| "挂钩状态已损坏".to_string())?;
  if guard.is_some() { return Err("模型加载器已启用，请先退出游戏".into()); }
  let dll = root.join("Profiles").join("XXMI").join("d3d11.dll");
  if !dll.is_file() {
    return Err("找不到 XXMI 核心 d3d11.dll。".to_string());
  }
  let loader = root.join("3dmloader.dll");
  if !loader.is_file() {
    return Err("找不到 3dmloader.dll。".to_string());
  }
  let module = unsafe { LoadLibraryW(wide(&loader).as_ptr()) };
  if module.is_null() {
    return Err("无法加载 3dmloader.dll。".to_string());
  }
  let address = unsafe { GetProcAddress(module, b"HookLibrary\0".as_ptr()) };
  if address.is_null() {
    unsafe { FreeLibrary(module) };
    return Err("3dmloader.dll 缺少 HookLibrary 入口。".to_string());
  }
  let function: HookLibrary = unsafe { std::mem::transmute(address) };
  let mut hook: *mut c_void = std::ptr::null_mut();
  let mut mutex: *mut c_void = std::ptr::null_mut();
  let dll_wide = wide(&dll);
  let previous = std::env::current_dir().ok();
  let _ = std::env::set_current_dir(dll.parent().unwrap_or(root));
  let result = unsafe { function(dll_wide.as_ptr(), &mut hook, &mut mutex) };
  if let Some(previous) = previous {
    let _ = std::env::set_current_dir(previous);
  }
  // HookLibrary acquires one core reference before returning 0, 300 or 400.
  // GetModuleHandleW borrows that reference; teardown releases it exactly once.
  let core = if result == 0 || result == 300 || result == 400 {
    unsafe { GetModuleHandleW(dll_wide.as_ptr()) }
  } else { std::ptr::null_mut() };
  if result != 0 || hook.is_null() {
    if !core.is_null() { unsafe { FreeLibrary(core) }; }
    if !mutex.is_null() {
      unsafe { CloseHandle(mutex) };
    }
    unsafe { FreeLibrary(module) };
    let reason = match result {
      100 => "已有其他 GIMI / XXMI 加载器运行，请先退出它。",
      200 => "无法加载 DLL，请检查文件完整性。",
      300 => "DLL 缺少所需入口。",
      400 => "无法设置加载钩子，请检查游戏与启动器的权限。",
      _ => "加载失败，请查看日志。",
    };
    return Err(reason.to_string());
  }
  *guard = Some(HookState { module, core, hook, mutex });
  Ok(())
}

