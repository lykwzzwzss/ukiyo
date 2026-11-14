//! Start with the desktop user's token when the launcher itself is elevated.
use std::{ffi::c_void, io, os::windows::ffi::OsStrExt, path::Path, process::Child};

struct Handle(isize);
impl Drop for Handle { fn drop(&mut self) { if self.0 != 0 { unsafe { CloseHandle(self.0 as *mut c_void); } } } }

pub enum GameProcess {
  Child(Child),
  Desktop { handle: DesktopProcess, pid: u32 },
}
pub struct DesktopProcess(Handle);
impl DesktopProcess {
  /// 用已有的进程句柄构造（例如 ShellExecuteEx 提权启动拿到的 hProcess）。
  pub fn from_handle(handle: isize) -> Self {
    Self(Handle(handle))
  }
}
impl GameProcess {
  pub fn id(&self) -> u32 { match self { Self::Child(child) => child.id(), Self::Desktop { pid, .. } => *pid } }
  pub fn try_wait(&mut self) -> io::Result<Option<String>> {
    match self {
      Self::Child(child) => child.try_wait().map(|status| status.map(|s| s.to_string())),
      Self::Desktop { handle, .. } => unsafe {
        match WaitForSingleObject(handle.0.0 as *mut c_void, 0) {
          258 => Ok(None),
          0 => {
            let mut code = 0;
            if GetExitCodeProcess(handle.0.0, &mut code) == 0 { Err(io::Error::last_os_error()) }
            else { Ok(Some(code.to_string())) }
          }
          _ => Err(io::Error::last_os_error()),
        }
      },
    }
  }
}

#[repr(C)]
struct StartupInfo {
  cb: u32, reserved: *mut u16, desktop: *mut u16, title: *mut u16,
  x: u32, y: u32, width: u32, height: u32, chars_x: u32, chars_y: u32,
  fill: u32, flags: u32, show: u16, reserved_size: u16, reserved_bytes: *mut u8,
  stdin: isize, stdout: isize, stderr: isize,
}
#[repr(C)]
struct ProcessInfo { process: isize, thread: isize, pid: u32, tid: u32 }

#[link(name = "user32")]
extern "system" { fn GetShellWindow() -> isize; fn GetWindowThreadProcessId(hwnd: *mut c_void, pid: *mut u32) -> u32; }
#[link(name = "kernel32")]
extern "system" {
  fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
  fn CloseHandle(handle: *mut c_void) -> i32;
  fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
  fn GetExitCodeProcess(handle: isize, code: *mut u32) -> i32;
}
#[link(name = "advapi32")]
extern "system" {
  fn OpenProcessToken(process: isize, access: u32, token: *mut isize) -> i32;
  fn DuplicateTokenEx(token: isize, access: u32, attributes: *const c_void, level: u32, kind: u32, result: *mut isize) -> i32;
  fn GetTokenInformation(token: isize, kind: u32, buffer: *mut c_void, length: u32, returned: *mut u32) -> i32;
  fn CreateProcessWithTokenW(token: isize, logon: u32, application: *const u16, command: *mut u16,
    flags: u32, environment: *const c_void, directory: *const u16, startup: *const StartupInfo, result: *mut ProcessInfo) -> i32;
}
#[link(name = "userenv")]
extern "system" {
  fn CreateEnvironmentBlock(environment: *mut *mut u16, token: isize, inherit: i32) -> i32;
  fn DestroyEnvironmentBlock(environment: *mut u16) -> i32;
}

fn wide(value: &std::ffi::OsStr) -> Result<Vec<u16>, String> {
  let mut value: Vec<u16> = value.encode_wide().collect();
  if value.contains(&0) { return Err("Launch path or arguments contain NUL".into()); }
  value.push(0);
  Ok(value)
}

pub fn spawn_as_desktop_user(path: &str, args: Option<&str>, directory: Option<&Path>, proxy: Option<&str>) -> Result<GameProcess, String> {
  unsafe {
    let shell = GetShellWindow();
    if shell == 0 { return Err("无法找到普通用户桌面，已取消非提权启动".into()); }
    let mut pid = 0;
    GetWindowThreadProcessId(shell as *mut c_void, &mut pid);
    let process = Handle(OpenProcess(0x1000, 0, pid) as isize);
    if process.0 == 0 { return Err(io::Error::last_os_error().to_string()); }
    let mut token = Handle(0);
    if OpenProcessToken(process.0, 0x000a, &mut token.0) == 0 { return Err(io::Error::last_os_error().to_string()); }
    let mut elevated: u32 = 0;
    let mut returned = 0;
    if GetTokenInformation(token.0, 20, &mut elevated as *mut _ as *mut c_void, 4, &mut returned) == 0 {
      return Err(io::Error::last_os_error().to_string());
    }
    if elevated != 0 { return Err("桌面进程也已提权，无法保证普通权限启动".into()); }
    let mut primary = Handle(0);
    if DuplicateTokenEx(token.0, 0x02000000, std::ptr::null(), 2, 1, &mut primary.0) == 0 {
      return Err(io::Error::last_os_error().to_string());
    }
    let mut original_env = std::ptr::null_mut();
    if CreateEnvironmentBlock(&mut original_env, primary.0, 0) == 0 { return Err(io::Error::last_os_error().to_string()); }
    let mut variables: Vec<Vec<u16>> = Vec::new();
    let mut current = original_env;
    while *current != 0 {
      let mut count = 0;
      while *current.add(count) != 0 { count += 1; }
      let variable = std::slice::from_raw_parts(current, count).to_vec();
      variables.push(variable);
      current = current.add(count + 1);
    }
    DestroyEnvironmentBlock(original_env);
    let environment = crate::game_environment::desktop_block(variables, proxy, true);
    let application = wide(std::ffi::OsStr::new(path))?;
    let mut command = wide(std::ffi::OsStr::new(&format!("\"{}\" {}", path, args.unwrap_or(""))))?;
    let directory = directory.map(|path| wide(path.as_os_str())).transpose()?;
    let mut desktop = wide(std::ffi::OsStr::new("winsta0\\default"))?;
    let mut startup: StartupInfo = std::mem::zeroed();
    startup.cb = std::mem::size_of::<StartupInfo>() as u32;
    startup.desktop = desktop.as_mut_ptr();
    let mut info: ProcessInfo = std::mem::zeroed();
    if CreateProcessWithTokenW(primary.0, 0, application.as_ptr(), command.as_mut_ptr(), 0x400,
      environment.as_ptr() as *const c_void, directory.as_ref().map_or(std::ptr::null(), |d| d.as_ptr()), &startup, &mut info) == 0 {
      return Err(format!("普通权限启动失败：{}", io::Error::last_os_error()));
    }
    let _thread = Handle(info.thread);
    Ok(GameProcess::Desktop { handle: DesktopProcess(Handle(info.process)), pid: info.pid })
  }
}
