use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// 旧版本配置里可能还有已废弃的字段，serde 会忽略它们。
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Configuration {
  pub game_install_path: Option<String>,
  pub last_ip: Option<String>,
  pub last_port: Option<String>,
  pub language: Option<String>,
  pub cert_generated: Option<bool>,
  pub debug_enabled: Option<bool>,
  pub patch_rsa: Option<bool>,
  pub use_internal_proxy: Option<bool>,
  pub wipe_login: Option<bool>,
  pub un_elevated: Option<bool>,
  pub launch_args: Option<String>,
  pub show_version: Option<bool>,
  pub gimi_enabled: Option<bool>,
  pub gimi_mods_path: Option<String>,
  pub gimi_fps: Option<u32>,
}

pub fn config_path() -> PathBuf {
  let mut path = tauri::api::path::data_dir().unwrap();
  path.push("cultivation");
  path.push("configuration.json");
  path
}

pub fn get_config() -> Configuration {
  let raw = std::fs::read_to_string(config_path()).unwrap_or_else(|_| "{}".into());
  serde_json::from_str(&raw).unwrap_or_default()
}
