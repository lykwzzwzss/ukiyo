use std::process::Command;

#[cfg(windows)]
use registry::{Data, Hive, Security};

/// 启动游戏：补丁预检、代理环境、控制台隐藏统一在这里处理。
///
/// `run_as_user` 为 true 时，如果启动器本身是提权的，会用桌面用户令牌启动游戏；
/// 启动器未提权时普通启动即可，因此不需要第二套“非提权”逻辑。
#[tauri::command]
pub async fn run_program(
  path: String,
  args: Option<String>,
  run_as_user: Option<bool>,
) -> Result<(), String> {
  let working_dir = std::path::Path::new(&path).parent().map(|p| p.to_path_buf());

  launch_game(
    &path,
    args.as_deref(),
    working_dir.as_deref(),
    run_as_user.unwrap_or(false),
  )
  .await
}


/// 启动器未提权、但游戏需要管理员权限时，用 ShellExecuteExW + runas 触发 UAC，
/// 并拿回进程句柄用于隐藏控制台 / 等待退出。
#[cfg(target_os = "windows")]
mod elevated_launch {
  use std::ffi::c_void;

  #[repr(C)]
  struct ShellExecuteInfoW {
    cb_size: u32,
    f_mask: u32,
    hwnd: *mut c_void,
    lp_verb: *const u16,
    lp_file: *const u16,
    lp_parameters: *const u16,
    lp_directory: *const u16,
    n_show: i32,
    h_inst_app: *mut c_void,
    lp_id_list: *mut c_void,
    lp_class: *const u16,
    hkey_class: *mut c_void,
    dw_hot_key: u32,
    h_icon: *mut c_void,
    h_process: *mut c_void,
  }

  #[link(name = "shell32")]
  extern "system" {
    fn ShellExecuteExW(info: *mut ShellExecuteInfoW) -> i32;
  }
  #[link(name = "kernel32")]
  extern "system" {
    fn GetProcessId(process: *mut c_void) -> u32;
  }

  fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
  }

  pub fn spawn(
    path: &str,
    args: Option<&str>,
    directory: Option<&std::path::Path>,
  ) -> Result<(isize, u32), String> {
    const SEE_MASK_NOCLOSEPROCESS: u32 = 0x0000_0040;
    const SEE_MASK_NOASYNC: u32 = 0x0000_0100;
    const SW_SHOWNORMAL: i32 = 1;

    let verb = wide("runas");
    let file = wide(path);
    let parameters = args.filter(|args| !args.trim().is_empty()).map(wide);
    let dir = directory.map(|path| wide(&path.to_string_lossy()));

    let mut info = ShellExecuteInfoW {
      cb_size: std::mem::size_of::<ShellExecuteInfoW>() as u32,
      f_mask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
      hwnd: std::ptr::null_mut(),
      lp_verb: verb.as_ptr(),
      lp_file: file.as_ptr(),
      lp_parameters: parameters.as_ref().map_or(std::ptr::null(), |value| value.as_ptr()),
      lp_directory: dir.as_ref().map_or(std::ptr::null(), |value| value.as_ptr()),
      n_show: SW_SHOWNORMAL,
      h_inst_app: std::ptr::null_mut(),
      lp_id_list: std::ptr::null_mut(),
      lp_class: std::ptr::null(),
      hkey_class: std::ptr::null_mut(),
      dw_hot_key: 0,
      h_icon: std::ptr::null_mut(),
      h_process: std::ptr::null_mut(),
    };

    let ok = unsafe { ShellExecuteExW(&mut info) };
    if ok == 0 {
      return Err(std::io::Error::last_os_error().to_string());
    }
    if info.h_process.is_null() {
      return Err("提权启动未返回进程句柄".into());
    }
    let pid = unsafe { GetProcessId(info.h_process) };
    Ok((info.h_process as isize, pid))
  }
}

#[cfg(target_os = "windows")]
async fn launch_game(
  path: &str,
  args: Option<&str>,
  working_dir: Option<&std::path::Path>,
  run_as_user: bool,
) -> Result<(), String> {
  use std::os::windows::process::CommandExt;

  let mut command = Command::new(path);
  let game_proxy = crate::proxy::game_proxy_url()?;
  crate::game_environment::apply(&mut command, game_proxy.as_deref(), run_as_user);

  if let Some(dir) = working_dir {
    command.current_dir(dir);
  }

  if let Some(args) = args {
    if !args.trim().is_empty() {
      command.raw_arg(args);
    }
  }

  crate::game_lifecycle::reserve()?;
  let preflight = async {
    let executable = std::path::Path::new(path);
    let version = crate::game_profile::detect(executable).ok();
    if version.map_or(false, |version| version.is_34()) {
      let server = crate::legacy34::installed_server(executable)?;
      crate::address_check::region_list(&server, game_proxy.as_deref()).await
    } else if let Some(proxy) = game_proxy.as_deref() {
      crate::address_check::via_proxy(proxy).await.map(|_| ())
    } else { Ok(()) }
  }.await;
  if let Err(error) = preflight {
    crate::game_lifecycle::failed();
    return Err(error);
  }
  if let Err(error) = crate::gimi::start_for_game(path.to_string()).await {
    crate::game_lifecycle::failed();
    return Err(format!("模型加载失败：{}", error));
  }
  let spawned = if run_as_user && is_elevated() {
    crate::windows_launch::spawn_as_desktop_user(path, args, working_dir, game_proxy.as_deref())
  } else {
    match command.spawn() {
      Ok(child) => Ok(crate::windows_launch::GameProcess::Child(child)),
      // 游戏清单要求管理员权限，而启动器本身未提权：交给 UAC 提权启动。
      Err(error) if error.raw_os_error() == Some(740) => {
        if game_proxy.is_some() || crate::config::get_config().gimi_enabled.unwrap_or(false) {
          // ShellExecute(run as administrator) cannot carry a child-only
          // environment, and an unelevated hook cannot target an elevated game.
          Err("游戏要求管理员权限。请以管理员身份运行浮世后重试，确保代理和模型加载正常。".into())
        } else { elevated_launch::spawn(path, args, working_dir).map(|(handle, pid)| {
          crate::windows_launch::GameProcess::Desktop {
            handle: crate::windows_launch::DesktopProcess::from_handle(handle),
            pid,
          }
        }) }
      }
      Err(error) => Err(error.to_string()),
    }
  };
  match spawned {
    Ok(mut child) => {
      let pid = child.id();
      crate::game_lifecycle::started();

      std::thread::spawn(move || {
        let started = std::time::Instant::now();
        loop {
          // 每次枚举前检查句柄，只有确认退出后才清理补丁。
          match child.try_wait() {
            Ok(None) => (),
            Ok(Some(status)) => {
              println!("Game process {} exited: {}", pid, status);
              break;
            }
            Err(e) => {
              eprintln!("Failed to query game process {}: {}", pid, e);
              std::thread::sleep(std::time::Duration::from_secs(1));
              continue;
            }
          }
          hide_console_window(pid);
          let interval = if started.elapsed() < std::time::Duration::from_secs(4) {
            20
          } else {
            200
          };
          std::thread::sleep(std::time::Duration::from_millis(interval));
        }
        crate::game_lifecycle::finished();
      });
      Ok(())
    }
    Err(e) => { crate::game_lifecycle::failed(); Err(format!("Failed to open program ({}): {}", path, e)) },
  }
}

#[cfg(target_os = "windows")]
mod console_window {
  const SW_HIDE: i32 = 0;

  #[link(name = "user32")]
  extern "system" {
    fn EnumWindows(
      callback: Option<unsafe extern "system" fn(*mut core::ffi::c_void, isize) -> i32>,
      lparam: isize,
    ) -> i32;
    fn GetWindowThreadProcessId(hwnd: *mut core::ffi::c_void, pid: *mut u32) -> u32;
    fn GetClassNameW(hwnd: *mut core::ffi::c_void, buffer: *mut u16, max_count: i32) -> i32;
    fn ShowWindow(hwnd: *mut core::ffi::c_void, cmd: i32) -> i32;
  }

  unsafe extern "system" fn hide_matching_console(
    hwnd: *mut core::ffi::c_void,
    lparam: isize,
  ) -> i32 {
    let target = lparam as u32;

    if target != 0 {
      let mut pid: u32 = 0;
      GetWindowThreadProcessId(hwnd, &mut pid);

      if pid == target {
        let mut buffer = [0u16; 64];
        let len = GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);

        if len > 0 {
          let class = String::from_utf16_lossy(&buffer[..len as usize]);

          if class == "ConsoleWindowClass" {
            ShowWindow(hwnd, SW_HIDE);
          }
        }
      }
    }

    1
  }

  pub fn hide_console_window(pid: u32) {
    unsafe {
      EnumWindows(Some(hide_matching_console), pid as isize);
    }
  }
}

#[cfg(target_os = "windows")]
use console_window::hide_console_window;

#[tauri::command]
pub fn open_in_browser(url: String) {
  // Open the URL in the default browser.
  match open::that(url) {
    Ok(_) => (),
    Err(e) => println!("Failed to open URL: {}", e),
  };
}

#[cfg(windows)]
#[tauri::command]
pub fn wipe_registry(exec_name: String) {
  // Fetch the 'Internet Settings' registry key.
  let settings =
    match Hive::CurrentUser.open(format!("Software\\miHoYo\\{}", exec_name), Security::Write) {
      Ok(s) => s,
      Err(e) => {
        println!("Error getting registry setting: {}", e);
        return;
      }
    };

  // Wipe login cache
  match settings.set_value(
    "MIHOYOSDK_ADL_PROD_OVERSEA_h1158948810",
    &Data::String("".parse().unwrap()),
  ) {
    Ok(_) => (),
    Err(e) => println!("Error wiping registry: {}", e),
  }

  match settings.set_value(
    "MIHOYOSDK_ADL_PROD_CN_h3123967166",
    &Data::String("".parse().unwrap()),
  ) {
    Ok(_) => (),
    Err(e) => println!("Error wiping registry: {}", e),
  }

  let hsr_settings =
    match Hive::CurrentUser.open("Software\\Cognosphere\\Star Rail".to_string(), Security::Write) {
      Ok(s) => s,
      Err(e) => {
        println!("Error getting registry setting: {}", e);
        return;
      }
    };

  match hsr_settings.set_value(
    "MIHOYOSDK_ADL_PROD_OVERSEA_h1158948810",
    &Data::String("".parse().unwrap()),
  ) {
    Ok(_) => (),
    Err(e) => println!("Error wiping registry: {}", e),
  }
}

#[tauri::command]
pub fn is_elevated() -> bool {
  is_elevated::is_elevated()
}

#[tauri::command]
pub fn get_platform() -> &'static str {
  std::env::consts::OS
}

/// 启动器安装目录（补丁等资源相对它查找）。
pub fn install_location() -> String {
  let mut exe_path = std::env::current_exe().unwrap();
  exe_path.pop();
  exe_path.to_str().unwrap().to_string()
}
