use serde::Serialize;
use std::{fs, io::Read, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ClientVersion {
  pub major: u16,
  pub minor: u16,
  pub release: u16,
}

impl ClientVersion {
  pub fn parse(text: &str) -> Result<Self, String> {
    let text = text.trim().trim_start_matches("CNRELWin").trim_start_matches("OSRELWin");
    let mut parts = text.split(['.', '_']);
    let major = parts.next().ok_or("Missing major version")?.parse::<u16>().map_err(|_| "Invalid major version")?;
    let minor = parts.next().ok_or("Missing minor version")?.parse::<u16>().map_err(|_| "Invalid minor version")?;
    let release = parts.next().and_then(|value| value.parse::<u16>().ok()).unwrap_or(0);
    if major == 0 || major > 99 || minor > 99 { return Err("Invalid game version".into()); }
    Ok(Self { major, minor, release })
  }

  pub fn is_34(self) -> bool { self.major == 3 && self.minor == 4 }

  pub fn patch_key(self) -> String { format!("{}.{}", self.major, self.minor) }

  pub fn patch_dll(self) -> String {
    if self.is_34() { "3.4/mhypbase.dll".into() }
    else if self.major >= 6 { "6version.dll".into() }
    else if self.major == 5 { "5version.dll".into() }
    else if self.major == 4 && self.minor > 0 { format!("4{}version.dll", self.minor) }
    else { "version.dll".into() }
  }

  pub fn injected_dll(self) -> &'static str {
    if self.is_34() { "mhypbase.dll" }
    else if self.major >= 5 { "Astrolabe.dll" }
    else { "version.dll" }
  }
}

pub fn detect(executable: &Path) -> Result<ClientVersion, String> {
  let parent = executable.parent().ok_or("Invalid game executable path")?;
  let stem = executable.file_stem().and_then(|s| s.to_str()).ok_or("Invalid game executable name")?;
  let assets = parent.join(format!("{}_Data", stem)).join("StreamingAssets");
  if let Ok(raw) = fs::read(assets.join("asb_settings.json")) {
    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&raw) {
      if let Some(version) = json.get("variance").and_then(|v| v.as_str()) {
        if let Ok(version) = ClientVersion::parse(version) { return Ok(version); }
      }
    }
  }
  if let Ok(file) = fs::File::open(assets.join("BinaryVersion.bytes")) {
    let mut bytes = Vec::new();
    file.take(128).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if let Some(raw) = bytes.get(3..) {
      let length = raw.iter().take_while(|byte| byte.is_ascii_digit() || **byte == b'.').count();
      if let Ok(raw) = std::str::from_utf8(&raw[..length]) {
        if let Ok(version) = ClientVersion::parse(raw) { return Ok(version); }
      }
    }
  }
  if let Ok(config) = ini::Ini::load_from_file(parent.join("config.ini")) {
    if let Some(version) = config.get_from(Some("General"), "game_version") {
      if let Ok(version) = ClientVersion::parse(version) { return Ok(version); }
    }
  }
  Err("无法从客户端文件识别游戏版本，请选择正确的 YuanShen.exe 或 GenshinImpact.exe".into())
}

#[tauri::command]
pub fn get_game_version(path: String) -> Result<ClientVersion, String> { detect(Path::new(&path)) }

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn recognizes_legacy_and_current_variance() {
    assert!(ClientVersion::parse("3.4_rel").unwrap().is_34());
    assert!(ClientVersion::parse("CNRELWin3.4.0").unwrap().is_34());
    assert_eq!(ClientVersion::parse("4.7_rel").unwrap().patch_key(), "4.7");
    assert_eq!(ClientVersion::parse("4.10.0").unwrap().minor, 10);
    for value in ["", "3", "NaN.4", "0.4", "3.999"] { assert!(ClientVersion::parse(value).is_err()); }
  }

  #[test]
  fn version_specific_assets_do_not_cross_install() {
    for (version, source, destination) in [
      ("3.4", "3.4/mhypbase.dll", "mhypbase.dll"),
      ("4.7", "47version.dll", "version.dll"),
      ("5.0", "5version.dll", "Astrolabe.dll"),
    ] {
      let version = ClientVersion::parse(version).unwrap();
      assert_eq!(version.patch_dll(), source);
      assert_eq!(version.injected_dll(), destination);
    }
  }

  #[test]
  fn detects_metadata_then_falls_back_to_binary_version() {
    let root = std::env::temp_dir().join(format!("ukiyo-version-{}", std::process::id()));
    let assets = root.join("YuanShen_Data").join("StreamingAssets");
    fs::create_dir_all(&assets).unwrap();
    let executable = root.join("YuanShen.exe");
    fs::write(assets.join("asb_settings.json"), br#"{"variance":"3.4_rel"}"#).unwrap();
    assert!(detect(&executable).unwrap().is_34());
    fs::write(assets.join("asb_settings.json"), b"broken json").unwrap();
    fs::write(assets.join("BinaryVersion.bytes"), b"\x03\x00\x003.4\xff").unwrap();
    assert!(detect(&executable).unwrap().is_34());
    // The test owns this uniquely named temporary directory.
    fs::remove_dir_all(root).unwrap();
  }
}
