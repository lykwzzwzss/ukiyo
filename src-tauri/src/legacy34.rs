use ini::{Ini, ParseOption};
use std::path::Path;

fn parse_ini(text: &str) -> Result<Ini, String> {
  Ini::load_from_str_opt(text, ParseOption { enabled_quote: false, enabled_escape: false }).map_err(|e| e.to_string())
}

pub fn server_address(text: &str) -> Result<String, String> {
  let uri: tauri::http::Uri = text.trim().trim_end_matches('/').parse().map_err(|_| "无效的 3.4 服务器地址")?;
  if !matches!(uri.scheme_str(), Some("http") | Some("https")) || uri.host().is_none()
    || uri.path() != "/" || uri.query().is_some() || uri.authority().map_or(true, |a| a.as_str().contains('@')) {
    return Err("3.4 服务器地址必须是 http(s)://主机:端口".into());
  }
  Ok(uri.to_string().trim_end_matches('/').to_string())
}

fn configured_ini(template: &str, server: &str, overseas: bool) -> Result<Vec<u8>, String> {
  let server = server_address(server)?;
  let mut ini = parse_ini(template)?;
  let raw = ini.get_from(Some("Value"), "ConfigChannel").ok_or("3.4 补丁缺少 ConfigChannel")?;
  let mut channel: serde_json::Value = serde_json::from_str(raw).map_err(|e| format!("3.4 ConfigChannel 无效：{}", e))?;
  let object = channel.as_object_mut().ok_or("ConfigChannel 必须是 JSON 对象")?;
  object.insert("ChannelName".into(), serde_json::json!(if overseas { "OSRELWin" } else { "CNRELWin" }));
  object.insert("DispatchConfigs".into(), serde_json::json!([{ "DispatchUrls": [format!("{}/query_region_list", server)] }]));
  ini.with_section(Some("Value")).set("ConfigChannel", channel.to_string()).set("ConfigBaseUrl", server);
  let mut bytes = Vec::new();
  ini.write_to_policy(&mut bytes, ini::EscapePolicy::Nothing).map_err(|e| e.to_string())?;
  Ok(bytes)
}

pub fn install(executable: &Path, server: &str, resources: &Path) -> Result<(), String> {
  let parent = executable.parent().ok_or("无效的游戏路径")?;
  let dll = std::fs::read(resources.join("mhypbase.dll")).map_err(|e| format!("缺少 3.4 专用 mhypbase.dll：{}", e))?;
  let template = std::fs::read_to_string(resources.join("mhypbase.ini")).map_err(|e| format!("缺少 3.4 专用 mhypbase.ini：{}", e))?;
  let overseas = executable.file_name().and_then(|name| name.to_str()).map_or(false, |name| name.eq_ignore_ascii_case("GenshinImpact.exe"));
  let ini = configured_ini(&template, server, overseas)?;
  crate::managed_patch::install_payloads(&[(parent.join("mhypbase.dll"), dll), (parent.join("mhypbase.ini"), ini)])
}

pub fn installed_server(executable: &Path) -> Result<String, String> {
  let text = std::fs::read_to_string(executable.parent().ok_or("无效的游戏路径")?.join("mhypbase.ini"))
    .map_err(|e| format!("无法读取 3.4 补丁配置：{}", e))?;
  let ini = parse_ini(&text)?;
  server_address(ini.get_from(Some("Value"), "ConfigBaseUrl").ok_or("3.4 补丁缺少 ConfigBaseUrl")?)
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn retains_patch_offsets_and_keys_while_retargeting_dispatch() {
    let template = include_str!("../patch/3.4/mhypbase.ini");
    let original = parse_ini(template).unwrap();
    for overseas in [false, true] {
      let bytes = configured_ini(template, "http://192.168.2.152:2891", overseas).unwrap();
      let result = parse_ini(std::str::from_utf8(&bytes).unwrap()).unwrap();
      assert_eq!(result.get_from(Some("Value"), "PublicRSAKey"), original.get_from(Some("Value"), "PublicRSAKey"));
      for section in ["Offset", "OSRELWin3.4.0", "CNRELWin3.4.0", "MD5ClientVersion"] {
        assert_eq!(result.section(Some(section)), original.section(Some(section)));
      }
      let channel: serde_json::Value = serde_json::from_str(result.get_from(Some("Value"), "ConfigChannel").unwrap()).unwrap();
      assert_eq!(channel["ChannelName"], if overseas { "OSRELWin" } else { "CNRELWin" });
      assert_eq!(channel["DispatchConfigs"][0]["DispatchUrls"][0], "http://192.168.2.152:2891/query_region_list");
      assert!(!String::from_utf8(bytes).unwrap().contains("192.168.126.129"));
    }
  }

  #[test]
  #[ignore = "Read-only integration check requiring a local 3.4 client and running server"]
  fn existing_34_client_and_region_server_read_only() {
    let executable = std::path::PathBuf::from(std::env::var_os("UKIYO_TEST_CLIENT").expect("Set UKIYO_TEST_CLIENT to the 3.4 executable"));
    let version = crate::game_profile::detect(&executable).unwrap();
    assert!(version.is_34());
    let local_dll = std::fs::read(executable.parent().unwrap().join("mhypbase.dll")).unwrap();
    assert_eq!(local_dll.as_slice(), include_bytes!("../patch/3.4/mhypbase.dll"));
    let server = installed_server(&executable).unwrap();
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    runtime.block_on(crate::address_check::region_list(&server, None)).unwrap();
  }
}
