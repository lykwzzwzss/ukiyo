use crate::config;
use crate::file_helpers;
use crate::system_helpers;
use std::path::PathBuf;

#[tauri::command]
pub async fn patch_game(newer_game: bool, version: String, server_addr: Option<String>) -> Result<bool, String> {
  let _ = newer_game;
  if crate::game_lifecycle::is_running() { return Err("游戏正在运行，不能更换补丁".into()); }
  let config = config::get_config();
  let executable = PathBuf::from(config.game_install_path.ok_or("尚未设置游戏路径")?);
  let detected = crate::game_profile::detect(&executable)?;
  let requested = if version.contains('.') { version.clone() } else { format!("{}{}", detected.major, detected.minor) };
  if version != "0" && version != detected.patch_key() && version != format!("{}{}", detected.major, detected.minor) {
    return Err(format!("客户端版本与补丁请求不一致：客户端 {}，请求 {}", detected.patch_key(), requested));
  }
  let resources = PathBuf::from(system_helpers::install_location()).join("patch");
  let parent = executable.parent().ok_or("无效的游戏路径")?;
  if detected.is_34() {
    let server = server_addr.unwrap_or_else(|| format!("http://{}:{}", config.last_ip.unwrap_or_else(|| "localhost".into()), config.last_port.unwrap_or_else(|| "443".into())));
    crate::legacy34::install(&executable, &server, &resources.join("3.4"))?;
    return Ok(true);
  }
  let source = resources.join(detected.patch_dll());
  let destination = parent.join(detected.injected_dll());
  if !source.is_file() { return Err(format!("缺少 {} 对应的补丁：{}", detected.patch_key(), source.display())); }
  if file_helpers::are_files_identical(&source.to_string_lossy(), &parent.join("mhypbase.dll").to_string_lossy()) { return Ok(true); }
  if !crate::managed_patch::install(&source, &destination) { return Err("安装补丁失败，请检查文件权限或先恢复之前的补丁".into()); }
  Ok(true)
}

#[tauri::command]
pub async fn unpatch_game() -> bool {
  if crate::game_lifecycle::is_running() { return false; }
  crate::managed_patch::cleanup()
}
