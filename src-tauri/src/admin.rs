#[cfg(windows)]
pub fn reopen_as_admin() {
  use std::ffi::{c_void, OsStr};
  use std::os::windows::ffi::OsStrExt;
  use std::process::exit;

  #[link(name = "shell32")]
  extern "system" {
    fn ShellExecuteW(
      window: *mut c_void,
      operation: *const u16,
      file: *const u16,
      parameters: *const u16,
      directory: *const u16,
      show_command: i32,
    ) -> *mut c_void;
  }

  let install = match std::env::current_exe() {
    Ok(install) => install,
    Err(error) => {
      eprintln!("Cannot locate launcher for elevation: {}", error);
      return;
    }
  };
  let operation: Vec<u16> = OsStr::new("runas").encode_wide().chain(Some(0)).collect();
  let executable: Vec<u16> = install.as_os_str().encode_wide().chain(Some(0)).collect();

  // Request elevation directly: no PowerShell process or console is created.
  // ShellExecuteW returns a value greater than 32 only when launch succeeds.
  let result = unsafe {
    ShellExecuteW(
      std::ptr::null_mut(), operation.as_ptr(), executable.as_ptr(),
      std::ptr::null(), std::ptr::null(), 1, // SW_SHOWNORMAL for the launcher GUI.
    )
  } as usize;
  if result > 32 {
    exit(0);
  }
  // If elevation is declined or fails, keep the existing launcher available.
  eprintln!("Launcher elevation did not succeed (ShellExecuteW code {}). Continuing without elevation.", result);
}
