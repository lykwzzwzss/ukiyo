#![cfg_attr(
  all(not(debug_assertions), target_os = "windows"),
  windows_subsystem = "windows"
)]

use args::{Args, ArgsError};
use file_helpers::dir_exists;
use proxy::set_proxy_addr;
use std::fs;
use std::io::Write;
use tauri::api::path::data_dir;
use tauri::async_runtime::block_on;

#[cfg(target_os = "windows")]
use crate::admin::reopen_as_admin;

#[cfg(target_os = "windows")]
extern "system" {
  fn GetConsoleWindow() -> *mut core::ffi::c_void;
  fn ShowWindow(hwnd: *mut core::ffi::c_void, n_cmd_show: i32) -> i32;
}
#[cfg(target_os = "windows")]
use system_helpers::is_elevated;

mod address_check;
mod admin;
mod config;
mod file_helpers;
mod game_environment;
mod game_lifecycle;
mod game_profile;
mod gimi;
mod lang;
mod legacy34;
mod managed_patch;
mod patch;
mod proxy;
mod system_helpers;
mod wallpapers;
#[cfg(windows)]
mod windows_launch;

fn try_flush() {
  std::io::stdout().flush().unwrap_or(())
}

async fn parse_args(inp: &Vec<String>) -> Result<Args, ArgsError> {
  let mut args = Args::new(
    "Cultivation",
    "Private server helper program for an Anime Game",
  );
  args.flag("h", "help", "Print various CLI args");
  args.flag("p", "proxy", "Start the proxy server");
  args.flag("G", "launch-game", "Launch the game");
  args.flag(
    "A",
    "no-admin",
    "Launch without requiring admin permissions",
  );
  args.flag(
    "g",
    "no-gui",
    "Run in CLI mode. Requires -A to be passed as well.",
  );
  args.flag(
    "P",
    "patch",
    "Patch your game before launching, with whatever your game version needs",
  );
  args.flag(
    "N",
    "non-elevated-game",
    "Launch the game without admin permissions",
  );
  args.option(
    "H",
    "host",
    "Set host to connect to (eg. 'localhost:443' or 'my.awesomeserver.com:6969)",
    "SERVER_HOST",
    getopts::Occur::Optional,
    None,
  );
  args.flag("Q", "gimi-prepare", "Prepare GIMI views and print a summary");
  args.option(
    "a",
    "game-args",
    "Arguments to pass to the game process, if launching it",
    r#""-opt-one -opt-two""#,
    getopts::Occur::Optional,
    None,
  );

  args.parse(inp).unwrap();

  let config = config::get_config();

  if args.value_of("help")? {
    println!("{}", args.full_usage());
    std::process::exit(0);
  }

  // Patch if needed
  if args.value_of("patch")? {
    if let Err(error) = patch::patch_game(false, "0".into(), None).await {
      eprintln!("{}", error);
    }
  }

  if args.value_of("gimi-prepare")? {
    let root = match gimi::cli_root() {
      Ok(root) => root,
      Err(error) => {
        eprintln!("{}", error);
        std::process::exit(1);
      }
    };
    let game = match config.game_install_path.clone().filter(|value| !value.trim().is_empty()) {
      Some(game) => game,
      None => {
        eprintln!("游戏路径未设置");
        std::process::exit(1);
      }
    };
    let mods = match config.gimi_mods_path.clone() {
      Some(path) if !path.trim().is_empty() => std::path::PathBuf::from(path),
      _ => root.join("Mods"),
    };
    match gimi::prepare_workspace(&root, std::path::Path::new(&game), &mods) {
      Ok(summary) => {
        println!("{}", serde_json::to_string_pretty(&summary).unwrap_or_default());
        std::process::exit(0);
      }
      Err(error) => {
        eprintln!("{}", error);
        std::process::exit(1);
      }
    }
  }

  if args.value_of("launch-game")? {
    let game_path = config.game_install_path;
    let game_args: String = args.value_of("game-args").unwrap_or(String::new());

    if let Some(game_path) = game_path {
      let run_as_user = args.value_of("non-elevated-game")?;
      if let Err(e) = system_helpers::run_program(game_path, Some(game_args), Some(run_as_user)).await {
        eprintln!("{}", e);
      }
    }
  }

  if args.value_of::<String>("host").is_ok() && !args.value_of::<String>("host")?.is_empty() {
    let host = args.value_of::<String>("host")?;
    if let Err(e) = set_proxy_addr(host) { eprintln!("{}", e); }
  }

  if args.value_of("proxy")? {
    println!("Starting proxy server...");
    let mut pathbuf = tauri::api::path::data_dir().unwrap();
    pathbuf.push("cultivation");
    pathbuf.push("ca");

    if let Err(e) = connect(8035, pathbuf.to_str().unwrap().to_string()).await { eprintln!("{}", e); }
  }

  Ok(args)
}

fn main() -> Result<(), ArgsError> {
  // 忽略 tauri dev / cargo 透传过来的构建参数，只保留应用自身参数
  let args: Vec<String> = std::env::args()
    .filter(|arg| arg != "--no-default-features" && arg != "--")
    .collect();
  let parsed_args = block_on(parse_args(&args)).unwrap();

  // Windows 调试构建会附带一个控制台窗口，启动游戏后启动器最小化，
  // 这个黑框就会留在桌面上。双击启动时把它隐藏；从终端运行时保留日志输出。
  #[cfg(target_os = "windows")]
  if !parsed_args.value_of("no-gui")? {
    unsafe {
      let console = GetConsoleWindow();
      if !console.is_null() {
        ShowWindow(console, 0);
      }
    }
  }

  #[cfg(target_os = "windows")]
  if !is_elevated() && !parsed_args.value_of("no-admin")? {
    println!("===============================================================================");
    println!("You running as a non-elevated user. Some stuff will almost definitely not work.");
    println!("===============================================================================");

    reopen_as_admin();
  }

  // Setup datadir/cultivation just in case something went funky and it wasn't made
  if !dir_exists(data_dir().unwrap().join("cultivation").to_str().unwrap()) {
    fs::create_dir_all(data_dir().unwrap().join("cultivation")).unwrap();
  }

  // Always set CWD to the location of the executable.
  let mut exe_path = std::env::current_exe().unwrap();
  exe_path.pop();
  std::env::set_current_dir(&exe_path).unwrap();

  // For disabled GUI
  ctrlc::set_handler(|| {
    gimi::shutdown();
    disconnect();
    block_on(patch::unpatch_game());
    std::process::exit(0);
  })
  .unwrap_or(());

  if !parsed_args.value_of("no-gui")? {
    tauri::Builder::default()
      .setup(|app| {
        game_lifecycle::initialize(app.handle());
        Ok(())
      })
      .invoke_handler(tauri::generate_handler![
        wallpapers::list_wallpapers,
        connect,
        disconnect,
        is_game_running,
        system_helpers::run_program,
        system_helpers::open_in_browser,
        system_helpers::is_elevated,
        system_helpers::wipe_registry,
        system_helpers::get_platform,
        patch::patch_game,
        game_profile::get_game_version,
        patch::unpatch_game,
        proxy::set_proxy_addr,
        proxy::generate_ca_files,
        file_helpers::dir_exists,
        file_helpers::does_file_exist,
        lang::get_lang,
        lang::get_languages,
        gimi::gimi_status,
        gimi::gimi_preload,
        gimi::gimi_refresh,
        gimi::gimi_reload,
        gimi::gimi_fps_status,
        gimi::gimi_toggle_mod,
        gimi::gimi_rename_mod,
        gimi::gimi_open_mods
      ])
      .on_window_event(|event| {
        if let tauri::WindowEvent::CloseRequested { .. } = event.event() {
          // Ensure all proxy stuff is handled
          gimi::shutdown();
          disconnect();
        }
      })
      .run(tauri::generate_context!())
      .expect("error while running tauri application");
  } else {
    try_flush();
    println!("Press enter or CTRL-C twice to quit...");
    std::io::stdin().read_line(&mut String::new()).unwrap();
  }

  // Always disconnect upon closing the program
  gimi::shutdown();
  disconnect();

  // Always unpatch game upon closing the program
  block_on(patch::unpatch_game());

  Ok(())
}

#[tauri::command]
fn is_game_running() -> bool {
  game_lifecycle::is_running()
}

#[tauri::command]
async fn connect(port: u16, certificate_path: String) -> Result<(), String> {
  proxy::start_managed_proxy(port, certificate_path).await
}

#[tauri::command]
fn disconnect() {
  if let Err(error) = proxy::stop_managed_proxy() { eprintln!("Proxy disconnect failed: {}", error); }
}
