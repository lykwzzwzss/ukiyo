//! Persist desired switches without renaming live resources or rebuilding INIs.
use super::mods::{self, ModEntry};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::{Path, PathBuf}};

#[derive(Default, Deserialize, Serialize)]
struct Pending {
  selections: BTreeMap<String, bool>,
}

fn file(root: &Path, directory: &Path) -> PathBuf {
  let directory = std::fs::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf());
  let id = super::versioned::digest(directory.to_string_lossy().to_lowercase().as_bytes());
  root.join("User/Selections").join(format!("{}.json", id))
}

fn read(root: &Path, directory: &Path) -> Result<Pending, String> {
  match std::fs::read(file(root, directory)) {
    Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| format!("无法读取暂存的模型选择：{}", error)),
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Pending::default()),
    Err(error) => Err(format!("无法读取暂存的模型选择：{}", error)),
  }
}

fn save(root: &Path, directory: &Path, pending: &Pending) -> Result<(), String> {
  super::storage::write_changed(&file(root, directory), serde_json::to_vec_pretty(pending).map_err(|error| error.to_string())?)
}

pub fn overlay(root: &Path, directory: &Path, entries: &mut [ModEntry]) -> Result<usize, String> {
  let pending = read(root, directory)?;
  let mut changes = 0;
  for entry in entries {
    if let Some(&enabled) = pending.selections.get(&entry.name.to_lowercase()) {
      changes += usize::from(entry.enabled != enabled);
      entry.enabled = enabled;
    }
  }
  Ok(changes)
}

pub fn has_pending(root: &Path, directory: &Path) -> Result<bool, String> {
  Ok(!read(root, directory)?.selections.is_empty())
}

pub fn stage(root: &Path, directory: &Path, folder: &str, enabled: bool) -> Result<usize, String> {
  mods::validate_folder(folder)?;
  let path = directory.join(folder);
  if !path.is_dir() || mods::is_link(&path) { return Err("模型目录已变化，请刷新列表后重试".into()); }
  let mut pending = read(root, directory)?;
  let name = mods::display_name(folder).to_lowercase();
  if enabled == !mods::is_disabled(folder) { pending.selections.remove(&name); }
  else { pending.selections.insert(name, enabled); }
  save(root, directory, &pending)?;
  Ok(pending.selections.len())
}

fn rollback(directory: &Path, changes: &[(String, String, bool)]) -> Result<(), String> {
  let mut errors = Vec::new();
  for (_, current, original_enabled) in changes.iter().rev() {
    if let Err(error) = mods::toggle_mod(directory, current, *original_enabled) { errors.push(error); }
  }
  if errors.is_empty() { Ok(()) } else { Err(errors.join("；")) }
}

/// Shared by the reload button and game launch. Rebuild exactly once on success;
/// on failure restore folder names and regenerate the previous loading view.
pub fn apply<T>(root: &Path, directory: &Path, mut rebuild: impl FnMut() -> Result<T, String>) -> Result<T, String> {
  let pending = read(root, directory)?;
  let entries = mods::scan_mods(directory)?;
  let mut changes = Vec::new();
  // Validate the complete batch before moving any folder.
  for entry in entries {
    let Some(&enabled) = pending.selections.get(&entry.name.to_lowercase()) else { continue };
    if entry.enabled == enabled { continue; }
    let target = if enabled { entry.name } else { format!("DISABLED_{}", entry.name) };
    if directory.join(&target).exists() { return Err(format!("目标模型目录已存在：{}", target)); }
    changes.push((entry.folder, target, entry.enabled));
  }
  for (completed, (source, _, old_enabled)) in changes.iter().enumerate() {
    if let Err(error) = mods::toggle_mod(directory, source, !old_enabled) {
      return match rollback(directory, &changes[..completed]) {
        Ok(()) => Err(error),
        Err(rollback) => Err(format!("{}；恢复模型目录失败：{}", error, rollback)),
      };
    }
  }
  match rebuild() {
    Ok(result) => {
      save(root, directory, &Pending::default()).map_err(|error| format!("模型配置已重载，但暂存状态保存失败：{}", error))?;
      Ok(result)
    }
    Err(error) => {
      if changes.is_empty() { return Err(error); }
      if let Err(rollback) = rollback(directory, &changes) { return Err(format!("{}；恢复模型目录失败：{}", error, rollback)); }
      if let Err(restore) = rebuild() { return Err(format!("{}；恢复原加载配置失败：{}", error, restore)); }
      Err(error)
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::atomic::{AtomicUsize, Ordering};
  static ID: AtomicUsize = AtomicUsize::new(0);
  fn fixture() -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("ukiyo-selection-{}-{}", std::process::id(), ID.fetch_add(1, Ordering::Relaxed)));
    let directory = root.join("Mods");
    for name in ["A", "B"] {
      std::fs::create_dir_all(directory.join(name)).unwrap();
      std::fs::write(directory.join(name).join("mod.ini"), "original").unwrap();
    }
    (root, directory)
  }
  #[test]
  fn choices_persist_without_touching_live_mods_and_batch_builds_once() {
    let (root, directory) = fixture();
    assert_eq!(stage(&root, &directory, "A", false).unwrap(), 1);
    assert_eq!(stage(&root, &directory, "B", false).unwrap(), 2);
    assert_eq!(std::fs::read_to_string(directory.join("A/mod.ini")).unwrap(), "original");
    assert!(directory.join("B").is_dir());
    let mut entries = mods::scan_mods(&directory).unwrap();
    assert_eq!(overlay(&root, &directory, &mut entries).unwrap(), 2);
    assert!(entries.iter().all(|item| !item.enabled));
    let mut calls = 0;
    apply(&root, &directory, || { calls += 1; assert!(directory.join("DISABLED_A").is_dir()); assert!(directory.join("DISABLED_B").is_dir()); Ok(()) }).unwrap();
    assert_eq!(calls, 1);
    assert_eq!(overlay(&root, &directory, &mut mods::scan_mods(&directory).unwrap()).unwrap(), 0);
    let _ = std::fs::remove_dir_all(root);
  }
  #[test]
  fn toggling_back_cancels_pending_and_rebuild_failure_retains_choices() {
    let (root, directory) = fixture();
    stage(&root, &directory, "A", false).unwrap();
    assert_eq!(stage(&root, &directory, "A", true).unwrap(), 0);
    stage(&root, &directory, "B", false).unwrap();
    let mut calls = 0;
    let result = apply(&root, &directory, || { calls += 1; if calls == 1 { Err("build failed".into()) } else { assert!(directory.join("B").is_dir()); Ok(()) } });
    assert_eq!(result.unwrap_err(), "build failed");
    assert_eq!(calls, 2);
    assert!(directory.join("B/mod.ini").is_file());
    assert_eq!(overlay(&root, &directory, &mut mods::scan_mods(&directory).unwrap()).unwrap(), 1);
    let _ = std::fs::remove_dir_all(root);
  }
  #[test]
  fn conflicts_do_not_partially_apply_and_other_workspaces_are_independent() {
    let (root, directory) = fixture();
    stage(&root, &directory, "A", false).unwrap();
    stage(&root, &directory, "B", false).unwrap();
    std::fs::create_dir(directory.join("DISABLED_B")).unwrap();
    assert!(apply(&root, &directory, || -> Result<(), String> { panic!("must not rebuild a conflicting batch") }).is_err());
    assert!(directory.join("A").is_dir());
    let other = root.join("OtherMods");
    std::fs::create_dir(&other).unwrap();
    assert!(read(&root, &other).unwrap().selections.is_empty());
    assert!(stage(&root, &directory, "../A", false).is_err());
    let _ = std::fs::remove_dir_all(root);
  }
}
