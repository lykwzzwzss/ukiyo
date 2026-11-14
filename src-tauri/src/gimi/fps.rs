use once_cell::sync::Lazy;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

const WH_GETMESSAGE: i32 = 3;
const PAGE_READWRITE: u32 = 0x04;
const FILE_MAP_ALL_ACCESS: u32 = 0xF001F;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const SYNCHRONIZE: u32 = 0x0010_0000;

#[link(name = "kernel32")]
extern "system" {
  fn CreateFileMappingW(handle: *mut c_void, attributes: *mut c_void, protect: u32, high: u32, low: u32, name: *const u16) -> *mut c_void;
  fn MapViewOfFile(mapping: *mut c_void, access: u32, high: u32, low: u32, bytes: usize) -> *mut c_void;
  fn UnmapViewOfFile(address: *const c_void) -> i32;
  fn CloseHandle(handle: *mut c_void) -> i32;
  fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
  fn QueryFullProcessImageNameW(process: *mut c_void, flags: u32, buffer: *mut u16, size: *mut u32) -> i32;
  fn LoadLibraryW(path: *const u16) -> *mut c_void;
  fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
  fn FreeLibrary(module: *mut c_void) -> i32;
  fn GetLastError() -> u32;
  fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
  fn OpenMutexW(access: u32, inherit: i32, name: *const u16) -> *mut c_void;
  fn OpenFileMappingW(access: u32, inherit: i32, name: *const u16) -> *mut c_void;
}

#[link(name = "user32")]
extern "system" {
  fn EnumWindows(callback: Option<unsafe extern "system" fn(*mut c_void, isize) -> i32>, argument: isize) -> i32;
  fn GetClassNameW(window: *mut c_void, buffer: *mut u16, size: i32) -> i32;
  fn GetWindowThreadProcessId(window: *mut c_void, pid: *mut u32) -> u32;
  fn SetWindowsHookExW(kind: i32, callback: *mut c_void, module: *mut c_void, thread: u32) -> *mut c_void;
  fn UnhookWindowsHookEx(hook: *mut c_void) -> i32;
  fn PostThreadMessageW(thread: u32, message: u32, wparam: usize, lparam: isize) -> i32;
}

struct Session {
  stop: Arc<AtomicBool>,
  thread: Option<JoinHandle<()>>,
  state: Arc<Mutex<String>>,
}

static SESSION: Lazy<Mutex<Option<Session>>> = Lazy::new(|| Mutex::new(None));

pub fn valid_target(fps: u32) -> bool {
  fps == 0 || (10..=1000).contains(&fps)
}

pub fn state() -> String {
  SESSION.lock().ok().and_then(|guard| guard.as_ref().map(|session| session.state.lock().map(|value| value.clone()).unwrap_or_default())).unwrap_or_default()
}

pub fn stop() {
  let mut guard = match SESSION.lock() {
    Ok(guard) => guard,
    Err(_) => return,
  };
  stop_session(&mut guard);
}

fn stop_session(slot: &mut Option<Session>) {
  if let Some(session) = slot.take() {
    session.stop.store(true, Ordering::SeqCst);
    if let Some(thread) = session.thread {
      let _ = thread.join();
    }
  }
}

pub fn start(root: &Path, game: &Path, fps: u32) -> Result<(), String> {
  if !valid_target(fps) {
    return Err("帧率必须为默认或 10–1000。".to_string());
  }
  let mut guard = SESSION.lock().map_err(|_| "帧率状态已损坏".to_string())?;
  stop_session(&mut guard);
  if fps == 0 {
    return Ok(());
  }
  let stub = root.join("Modules").join("FpsUnlock").join("UnlockerStub.dll");
  if !stub.is_file() {
    return Err("帧率解锁组件缺失，请重新解压完整启动器。".to_string());
  }
  let stop_flag = Arc::new(AtomicBool::new(false));
  let state = Arc::new(Mutex::new("等待帧率解锁".to_string()));
  let game_path = game.to_path_buf();
  let stub_path = stub;
  let stop_clone = stop_flag.clone();
  let state_clone = state.clone();
  let thread = std::thread::Builder::new()
    .name("ukiyo-fps".to_string())
    .spawn(move || worker(game_path, stub_path, fps, stop_clone, state_clone))
    .map_err(|error| format!("无法启动帧率线程：{}", error))?;
  *guard = Some(Session { stop: stop_flag, thread: Some(thread), state });
  Ok(())
}

fn wide(path: &Path) -> Vec<u16> {
  use std::os::windows::ffi::OsStrExt;
  path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

fn worker(game: PathBuf, stub: PathBuf, fps: u32, stop: Arc<AtomicBool>, state: Arc<Mutex<String>>) {
  while !stop.load(Ordering::SeqCst) {
    if let Some((pid, window)) = find_window(&game) {
      set_state(&state, "正在解锁帧率");
      if let Err(reason) = attach(pid, window, &stub, fps, &stop, &state) {
        set_state(&state, &format!("帧率解锁失败：{}", reason));
        return;
      }
      if !stop.load(Ordering::SeqCst) {
        set_state(&state, "等待游戏启动");
      }
    }
    for _ in 0..5 {
      if stop.load(Ordering::SeqCst) {
        return;
      }
      std::thread::sleep(std::time::Duration::from_millis(50));
    }
  }
}

fn set_state(state: &Arc<Mutex<String>>, value: &str) {
  if let Ok(mut guard) = state.lock() {
    *guard = value.to_string();
  }
}

fn process_path(pid: u32) -> Option<PathBuf> {
  unsafe {
    let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if process.is_null() {
      return None;
    }
    let mut buffer = [0u16; 1024];
    let mut size = buffer.len() as u32;
    let ok = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut size);
    CloseHandle(process);
    if ok == 0 {
      return None;
    }
    Some(PathBuf::from(String::from_utf16_lossy(&buffer[..size as usize])))
  }
}

struct Search {
  target: PathBuf,
  found: Option<(u32, *mut c_void)>,
}

struct ProcessHandle(*mut c_void);
impl Drop for ProcessHandle {
  fn drop(&mut self) { unsafe { CloseHandle(self.0); } }
}

unsafe fn new_mapping(name: &[u16]) -> Result<*mut c_void, String> {
  let mapping = CreateFileMappingW(!0isize as *mut c_void, std::ptr::null_mut(), PAGE_READWRITE, 0, 4096, name.as_ptr());
  let error = GetLastError();
  if mapping.is_null() { return Err("无法创建帧率共享内存。".into()); }
  if error == 183 {
    CloseHandle(mapping);
    return Err("另一个帧率解锁器仍在工作，请关闭它并重启游戏。".into());
  }
  Ok(mapping)
}

unsafe fn conflicts() -> bool {
  let mutex: Vec<u16> = "286B345F-A2EB-4FF3-83E9-2DD83B87694A".encode_utf16().chain(Some(0)).collect();
  let handle = OpenMutexW(SYNCHRONIZE, 0, mutex.as_ptr());
  let error = GetLastError();
  if !handle.is_null() { CloseHandle(handle); return true; }
  if error == 5 { return true; }
  let name: Vec<u16> = "Global\\2DE95FDC-6AB7-4593-BFE6-760DD4AB422B".encode_utf16().chain(Some(0)).collect();
  let handle = OpenFileMappingW(4, 0, name.as_ptr());
  let error = GetLastError();
  if !handle.is_null() { CloseHandle(handle); return true; }
  error == 5
}

unsafe extern "system" fn enum_window(window: *mut c_void, argument: isize) -> i32 {
  let search = &mut *(argument as *mut Search);
  let mut pid = 0u32;
  GetWindowThreadProcessId(window, &mut pid);
  if pid == 0 {
    return 1;
  }
  let mut class = [0u16; 256];
  let length = GetClassNameW(window, class.as_mut_ptr(), class.len() as i32);
  if length <= 0 {
    return 1;
  }
  if String::from_utf16_lossy(&class[..length as usize]) != "UnityWndClass" {
    return 1;
  }
  if let Some(path) = process_path(pid) {
    if path.to_string_lossy().eq_ignore_ascii_case(&search.target.to_string_lossy()) {
      search.found = Some((pid, window));
      return 0;
    }
  }
  1
}

fn find_window(game: &Path) -> Option<(u32, *mut c_void)> {
  let mut search = Search { target: game.to_path_buf(), found: None };
  unsafe {
    EnumWindows(Some(enum_window), &mut search as *mut Search as isize);
  }
  search.found
}

pub(super) fn game_has_window(game: &Path) -> bool { find_window(game).is_some() }

fn attach(
  pid: u32,
  window: *mut c_void,
  stub: &Path,
  fps: u32,
  stop: &Arc<AtomicBool>,
  state: &Arc<Mutex<String>>,
) -> Result<(), String> {
  let name: Vec<u16> = "Local\\GIMI34.FpsUnlock.IPC".encode_utf16().chain(std::iter::once(0)).collect();
  unsafe {
    let process = OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if process.is_null() { return Err("无法访问游戏进程，请检查启动权限。".into()); }
    let process = ProcessHandle(process);
    if conflicts() { return Err("检测到其他帧率解锁组件，请先关闭它并重启游戏。".into()); }
    let mapping = new_mapping(&name)?;
    let view = MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, 4096) as *mut u8;
    if view.is_null() {
      CloseHandle(mapping);
      return Err("无法创建帧率共享内存。".to_string());
    }
    std::ptr::write_volatile(view, 0);
    std::ptr::write_volatile(view.add(4) as *mut u32, fps);
    std::ptr::write_volatile(view.add(8), 0);
    std::ptr::write_volatile(view.add(9), 0);

    let module = LoadLibraryW(wide(stub).as_ptr());
    if module.is_null() {
      cleanup(view, mapping, std::ptr::null_mut(), std::ptr::null_mut());
      return Err("无法加载帧率组件。".to_string());
    }
    let callback = GetProcAddress(module, b"WndProc\0".as_ptr());
    if callback.is_null() {
      cleanup(view, mapping, module, std::ptr::null_mut());
      return Err("帧率组件缺少 WndProc 入口。".to_string());
    }
    let mut window_pid = 0u32;
    let thread = GetWindowThreadProcessId(window, &mut window_pid);
    if thread == 0 || window_pid != pid {
      cleanup(view, mapping, module, std::ptr::null_mut());
      return Err("游戏窗口已关闭，请重试。".to_string());
    }
    let hook = SetWindowsHookExW(WH_GETMESSAGE, callback, module, thread);
    if hook.is_null() {
      cleanup(view, mapping, module, std::ptr::null_mut());
      return Err("无法启用帧率解锁。".to_string());
    }
    if PostThreadMessageW(thread, 0, 0, 0) == 0 {
      let _ = UnhookWindowsHookEx(hook);
      cleanup(view, mapping, module, std::ptr::null_mut());
      return Err("无法通知游戏窗口。".to_string());
    }
    let mut ready = false;
    for _ in 0..400 {
      if stop.load(Ordering::SeqCst) || WaitForSingleObject(process.0, 0) == 0 {
        break;
      }
      match std::ptr::read_volatile(view) {
        2 => {
          ready = true;
          break;
        }
        1 => {
          let _ = UnhookWindowsHookEx(hook);
          reset_and_cleanup(view, mapping, module);
          return Err("未找到帧率限制位置，当前客户端可能不兼容。".to_string());
        }
        _ => {}
      }
      std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let _ = UnhookWindowsHookEx(hook);
    if !ready {
      reset_and_cleanup(view, mapping, module);
      if stop.load(Ordering::SeqCst) || WaitForSingleObject(process.0, 0) == 0 { return Ok(()); }
      return Err("帧率组件响应超时，请重启游戏后重试。".into());
    }
    if ready {
      set_state(state, &format!("目标 {} FPS", fps));
    }
    while !stop.load(Ordering::SeqCst) {
      if WaitForSingleObject(process.0, 0) == 0 {
        break;
      }
      std::thread::sleep(std::time::Duration::from_millis(250));
    }
    reset_and_cleanup(view, mapping, module);
    Ok(())
  }
}

unsafe fn reset_and_cleanup(view: *mut u8, mapping: *mut c_void, module: *mut c_void) {
  if !view.is_null() {
    std::ptr::write_volatile(view.add(4) as *mut u32, 60);
    UnmapViewOfFile(view as *const c_void);
  }
  if !mapping.is_null() {
    CloseHandle(mapping);
  }
  if !module.is_null() {
    FreeLibrary(module);
  }
}

unsafe fn cleanup(view: *mut u8, mapping: *mut c_void, module: *mut c_void, hook: *mut c_void) {
  if !hook.is_null() {
    let _ = UnhookWindowsHookEx(hook);
  }
  reset_and_cleanup(view, mapping, module);
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn validates_targets() {
    assert!(valid_target(0));
    assert!(valid_target(10));
    assert!(valid_target(1000));
    assert!(!valid_target(9));
    assert!(!valid_target(1001));
  }

  #[test]
  fn existing_ipc_is_rejected_without_overwriting_it() {
    let name: Vec<u16> = format!("Local\\UkiyoFpsTest-{}", std::process::id()).encode_utf16().chain(Some(0)).collect();
    unsafe {
      let mapping = new_mapping(&name).unwrap();
      let view = MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, 4096) as *mut u32;
      assert!(!view.is_null());
      std::ptr::write_volatile(view, 123);
      assert!(new_mapping(&name).is_err());
      assert_eq!(std::ptr::read_volatile(view), 123);
      UnmapViewOfFile(view as *const c_void);
      CloseHandle(mapping);
    }
  }
}
