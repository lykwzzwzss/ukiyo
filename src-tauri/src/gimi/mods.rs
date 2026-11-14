use serde::Serialize;
use std::fs;
use std::path::Path;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModEntry {
  pub folder: String,
  pub name: String,
  pub display_name: String,
  pub enabled: bool,
  pub ini_count: usize,
  pub compatibility: String,
}

fn disabled_prefix(folder: &str) -> Option<usize> {
  if !folder.to_ascii_lowercase().starts_with("disabled") {
    return None;
  }
  let bytes = folder.as_bytes();
  let mut index = "disabled".len();
  while index < bytes.len() && (bytes[index] == b'_' || bytes[index] == b'-' || bytes[index] == b' ') {
    index += 1;
  }
  Some(index)
}

pub fn is_disabled(folder: &str) -> bool {
  disabled_prefix(folder).is_some()
}

pub fn display_name(folder: &str) -> String {
  match disabled_prefix(folder) {
    Some(index) => {
      let name = folder[index..].trim();
      if name.is_empty() { folder.to_string() } else { name.to_string() }
    }
    None => folder.to_string(),
  }
}

fn count_ini(directory: &Path, depth: usize) -> usize {
  if depth > 8 {
    return 0;
  }
  let entries = match fs::read_dir(directory) {
    Ok(entries) => entries,
    Err(_) => return 0,
  };
  let mut count = 0;
  for entry in entries.flatten() {
    if is_link(&entry.path()) { continue; }
    let path = entry.path();
    let name = entry.file_name().to_string_lossy().into_owned();
    if name.to_ascii_lowercase().starts_with("disabled") {
      continue;
    }
    if path.is_dir() {
      count += count_ini(&path, depth + 1);
    } else if path.extension().and_then(|value| value.to_str()).map(|value| value.eq_ignore_ascii_case("ini")).unwrap_or(false) {
      count += 1;
    }
  }
  count
}

pub fn is_link(path: &Path) -> bool {
  use std::os::windows::fs::MetadataExt;
  std::fs::symlink_metadata(path).map_or(true, |metadata| metadata.file_attributes() & 0x400 != 0)
}

pub fn scan_mods(directory: &Path) -> Result<Vec<ModEntry>, String> {
  let entries = match fs::read_dir(directory) {
    Ok(entries) => entries,
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
    Err(error) => return Err(format!("无法读取 Mods 目录：{}", error)),
  };
  let mut mods = Vec::new();
  for entry in entries.flatten() {
    let folder = entry.file_name().to_string_lossy().into_owned();
    let path = entry.path();
    let file_type = match entry.file_type() {
      Ok(value) => value,
      Err(_) => continue,
    };
    if !file_type.is_dir() || is_link(&path) {
      continue;
    }
    if folder.eq_ignore_ascii_case("DISABLED_GIMI_Config") {
      continue;
    }
    mods.push(ModEntry {
      enabled: !is_disabled(&folder),
      name: display_name(&folder),
      display_name: display_name(&folder),
      ini_count: count_ini(&path, 0),
      folder,
      compatibility: String::new(),
    });
  }
  mods.sort_by(|left, right| {
    right.enabled.cmp(&left.enabled)
      .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
  });
  Ok(mods)
}

pub fn validate_folder(folder: &str) -> Result<(), String> {
  if folder.is_empty() || folder.contains(['/', '\\', ':']) || folder.ends_with(['.', ' ']) || folder == "." || folder == ".." {
    return Err("模组名称无效".to_string());
  }
  Ok(())
}

pub fn toggle_mod(directory: &Path, folder: &str, enabled: bool) -> Result<(), String> {
  validate_folder(folder)?;
  let source = directory.join(folder);
  if !source.is_dir() || is_link(&source) {
    return Err(format!("模组目录不存在：{}", folder));
  }
  let display = display_name(folder);
  if enabled && is_disabled(&display) { return Err("模组目录缺少有效名称".into()); }
  let target_name = if enabled { display.clone() } else { format!("DISABLED_{}", display) };
  if folder == target_name {
    return Ok(());
  }
  let target = directory.join(&target_name);
  if target.exists() {
    return Err(format!("目标目录已存在：{}", target_name));
  }
  fs::rename(&source, &target).map_err(|error| format!("切换模组失败：{}", error))
}

pub fn detect_version(executable: &Path) -> Option<String> {
  let stem = executable.file_stem()?.to_str()?;
  let parent = executable.parent()?;
  let data = parent.join(format!("{}_Data", stem)).join("globalgamemanagers");
  let metadata = fs::metadata(&data).ok()?;
  if metadata.len() > 16 * 1024 * 1024 {
    return None;
  }
  let bytes = fs::read(&data).ok()?;
  let text = String::from_utf8_lossy(&bytes);
  let mut found: Vec<String> = Vec::new();
  for version in ["3.4", "4.7"] {
    let needle = format!("{}.0_", version);
    let mut start = 0;
    while let Some(offset) = text[start..].find(&needle) {
      let index = start + offset;
      let before_ok = index == 0 || !text.as_bytes()[index - 1].is_ascii_digit();
      let after = index + needle.len();
      let after_ok = text[after..].chars().next().map(|value| value.is_ascii_digit()).unwrap_or(false);
      if before_ok && after_ok && !found.iter().any(|value| value == version) {
        found.push(version.to_string());
      }
      start = after;
    }
  }
  if found.len() == 1 {
    found.pop()
  } else {
    None
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn strips_disabled_prefixes_like_gimi() {
    assert_eq!(display_name("Ayaka"), "Ayaka");
    assert_eq!(display_name("DISABLED_Ayaka"), "Ayaka");
    assert_eq!(display_name("disabled-Ayaka"), "Ayaka");
    assert_eq!(display_name("Disabled  Ayaka"), "Ayaka");
    assert!(is_disabled("DISABLED_x"));
    assert!(!is_disabled("Ayaka"));
  }

  #[test]
  fn detects_version_from_build_metadata() {
    let root = std::env::temp_dir().join(format!("ukiyo-gimi-version-{}", std::process::id()));
    let data = root.join("GenshinImpact_Data");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("globalgamemanagers"), "\0 4.7.0_1234567 \0 4.7.0_7654321 \0").unwrap();
    assert_eq!(detect_version(&root.join("GenshinImpact.exe")).as_deref(), Some("4.7"));
    fs::write(data.join("globalgamemanagers"), "3.4.0_11 4.7.0_22").unwrap();
    assert_eq!(detect_version(&root.join("GenshinImpact.exe")), None);
    let _ = fs::remove_dir_all(&root);
  }

  #[test]
  fn toggles_folder_names() {
    let root = std::env::temp_dir().join(format!("ukiyo-gimi-mods-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("Ayaka")).unwrap();
    fs::write(root.join("Ayaka").join("mod.ini"), "x").unwrap();
    let list = scan_mods(&root).unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].enabled);
    assert_eq!(list[0].ini_count, 1);
    toggle_mod(&root, "Ayaka", false).unwrap();
    assert!(root.join("DISABLED_Ayaka").is_dir());
    toggle_mod(&root, "DISABLED_Ayaka", true).unwrap();
    assert!(root.join("Ayaka").is_dir());
    let _ = fs::remove_dir_all(&root);
  }
}
