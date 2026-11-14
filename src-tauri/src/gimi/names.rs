//! Editable display names are separate from source folders and runtime identity.
use super::{mods, storage, versioned::digest};
use std::{collections::BTreeMap, path::{Path, PathBuf}};

type Names = BTreeMap<String, String>;

fn file(root: &Path, directory: &Path) -> PathBuf {
  let directory = std::fs::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf());
  root.join("User/ModNames").join(format!("{}.json", digest(directory.to_string_lossy().to_lowercase().as_bytes())))
}

fn read(root: &Path, directory: &Path) -> Result<Names, String> {
  match std::fs::read(file(root, directory)) {
    Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| format!("无法读取模型名称：{}", error)),
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Names::new()),
    Err(error) => Err(format!("无法读取模型名称：{}", error)),
  }
}

pub fn overlay(root: &Path, directory: &Path, entries: &mut [mods::ModEntry]) -> Result<(), String> {
  let names = read(root, directory)?;
  for entry in entries {
    entry.display_name = names.get(&entry.name.to_lowercase()).cloned().unwrap_or_else(|| entry.name.clone());
  }
  Ok(())
}

pub fn rename(root: &Path, directory: &Path, folder: &str, value: &str) -> Result<String, String> {
  mods::validate_folder(folder)?;
  let source = directory.join(folder);
  if folder.eq_ignore_ascii_case("DISABLED_GIMI_Config") || !source.is_dir() || mods::is_link(&source) {
    return Err("模型目录已变化，请刷新列表后重试".into());
  }
  let value = value.trim();
  if value.is_empty() { return Err("名称不能为空".into()); }
  if value.chars().count() > 100 || value.chars().any(char::is_control) {
    return Err("名称最多 100 个字符，不能包含换行或控制字符".into());
  }
  let original = mods::display_name(folder);
  let key = original.to_lowercase();
  let mut names = read(root, directory)?;
  // Only inspect top-level names, never rescan the entire resource tree to edit a label.
  for entry in std::fs::read_dir(directory).map_err(|error| error.to_string())? {
    let entry = entry.map_err(|error| error.to_string())?;
    let other = entry.file_name().to_string_lossy().into_owned();
    if !entry.path().is_dir() || mods::is_link(&entry.path()) || other.eq_ignore_ascii_case("DISABLED_GIMI_Config") { continue; }
    let other_name = mods::display_name(&other);
    let other_key = other_name.to_lowercase();
    if other_key == key { continue; }
    if names.get(&other_key).unwrap_or(&other_name).to_lowercase() == value.to_lowercase() {
      return Err("已有同名模型，请换一个名称".into());
    }
  }
  if value == original { names.remove(&key); }
  else { names.insert(key, value.to_string()); }
  storage::write_changed(&file(root, directory), serde_json::to_vec_pretty(&names).map_err(|error| error.to_string())?)?;
  Ok(value.to_string())
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn display_rename_preserves_resources_pending_choices_and_disabled_identity() {
    let root = std::env::temp_dir().join(format!("ukiyo-mod-names-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let directory = root.join("Mods");
    for name in ["A", "B"] {
      std::fs::create_dir_all(directory.join(name)).unwrap();
      std::fs::write(directory.join(name).join("mod.ini"), "untouched").unwrap();
    }
    super::super::selection::stage(&root, &directory, "A", false).unwrap();
    assert_eq!(rename(&root, &directory, "A", "  新名称  ").unwrap(), "新名称");
    assert_eq!(std::fs::read_to_string(directory.join("A/mod.ini")).unwrap(), "untouched");
    let mut entries = mods::scan_mods(&directory).unwrap();
    assert_eq!(super::super::selection::overlay(&root, &directory, &mut entries).unwrap(), 1);
    overlay(&root, &directory, &mut entries).unwrap();
    let a = entries.iter().find(|entry| entry.folder == "A").unwrap();
    assert_eq!(a.name, "A");
    assert_eq!(a.display_name, "新名称");
    assert!(!a.enabled);
    assert!(rename(&root, &directory, "B", "新名称").is_err());
    assert!(rename(&root, &directory, "A", " \n ").is_err());
    assert!(rename(&root, &directory, "../A", "坏名称").is_err());
    mods::toggle_mod(&directory, "A", false).unwrap();
    let mut entries = mods::scan_mods(&directory).unwrap();
    overlay(&root, &directory, &mut entries).unwrap();
    assert_eq!(entries.iter().find(|entry| entry.folder == "DISABLED_A").unwrap().display_name, "新名称");
    rename(&root, &directory, "DISABLED_A", "A").unwrap();
    assert!(read(&root, &directory).unwrap().is_empty());
    let other = root.join("OtherMods");
    std::fs::create_dir_all(other.join("A")).unwrap();
    rename(&root, &directory, "DISABLED_A", "又一个名称").unwrap();
    assert!(read(&root, &other).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(root);
  }
}
