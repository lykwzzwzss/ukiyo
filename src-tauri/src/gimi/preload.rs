//! One active prepared view: switching versions also replaces shared catalog files.
use sha2::{Digest, Sha256};
use std::path::Path;

pub struct Cache<T> { entry: Option<(String, String, T)> }
impl<T: Clone> Cache<T> {
  pub fn new() -> Self { Self { entry: None } }
  pub fn clear(&mut self) { self.entry = None; }
  pub fn get(&self, root: &Path, game: &Path, mods: &Path) -> Option<T> {
    let (inputs, outputs, value) = self.entry.as_ref()?;
    if &input_stamp(root, game, mods).ok()? != inputs || &output_stamp(root, mods).ok()? != outputs { return None; }
    Some(value.clone())
  }
  pub fn store(&mut self, root: &Path, game: &Path, mods: &Path, before: &str, value: T) {
    self.entry = None;
    // Don't reuse a view if source files changed while it was being generated.
    if let (Ok(after), Ok(outputs)) = (input_stamp(root, game, mods), output_stamp(root, mods)) {
      if after == before { self.entry = Some((after, outputs, value)); }
    }
  }
}

fn walk(path: &Path, hash: &mut Sha256) -> Result<(), String> {
  use std::os::windows::fs::MetadataExt;
  hash.update(path.to_string_lossy().as_bytes());
  let metadata = match std::fs::symlink_metadata(path) {
    Ok(value) => value,
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => { hash.update(b"missing\0"); return Ok(()); }
    Err(error) => return Err(format!("无法检查 {}：{}", path.display(), error)),
  };
  if metadata.file_attributes() & 0x400 != 0 { hash.update(b"link\0"); return Ok(()); }
  if metadata.is_dir() {
    hash.update(b"directory\0");
    let mut entries = std::fs::read_dir(path).map_err(|error| error.to_string())?
      .collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
      let name = entry.file_name().to_string_lossy().to_lowercase();
      if name == "disabled_gimi_config" || name == ".git" || name.ends_with(".log") || name.ends_with("_log.txt") { continue; }
      walk(&entry.path(), hash)?;
    }
  } else {
    hash.update(metadata.len().to_le_bytes());
    let ticks = metadata.modified().map_err(|error| error.to_string())?
      .duration_since(std::time::UNIX_EPOCH).map_err(|error| error.to_string())?.as_nanos();
    hash.update(ticks.to_le_bytes());
    let extension = path.extension().and_then(|value| value.to_str()).unwrap_or_default();
    if extension.eq_ignore_ascii_case("ini") || extension.eq_ignore_ascii_case("json") {
      hash.update(std::fs::read(path).map_err(|error| error.to_string())?);
    }
  }
  Ok(())
}

pub fn input_stamp(root: &Path, game: &Path, mods: &Path) -> Result<String, String> {
  let mut hash = Sha256::new();
  hash.update(root.to_string_lossy().as_bytes());
  hash.update(mods.to_string_lossy().as_bytes());
  hash.update(game.to_string_lossy().as_bytes());
  hash.update(super::detect_version(game).unwrap_or_default().as_bytes());
  for path in [mods.to_path_buf(), root.join("Modules"), root.join("Profiles/XXMI/template.ini"), root.join("Profiles/XXMI/ShaderFixes"), root.join("Profiles/XXMI/ShaderFixesWithOffset"), root.join("Profiles/XXMI/d3d11.dll"), root.join("3dmloader.dll")] {
    walk(&path, &mut hash)?;
  }
  Ok(format!("{:x}", hash.finalize()))
}

fn output_stamp(root: &Path, mods: &Path) -> Result<String, String> {
  let mut hash = Sha256::new();
  for relative in ["Profiles/XXMI/d3dx.ini", "Profiles/XXMI/mod-catalog.ini", "Profiles/XXMI/d3dx_user.ini",
    "User/VersionViews/XXMI", "User/RuntimeShaders", "User/addons.ini", "User/mod-switching.ini", "User/mod-switching.json",
    "User/ModState", "User/shared-mod-state.json", "User/LegacyResources"] {
    walk(&root.join(relative), &mut hash)?;
  }
  walk(&mods.join("DISABLED_GIMI_Config/Buffers"), &mut hash)?;
  Ok(format!("{:x}", hash.finalize()))
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn reuse_checks_version_sources_outputs_and_persistent_state() {
    let root = std::env::temp_dir().join(format!("ukiyo-preload-{}", std::process::id()));
    let mods = root.join("Mods");
    std::fs::create_dir_all(mods.join("A")).unwrap();
    std::fs::create_dir_all(root.join("User/VersionViews/XXMI/3.4")).unwrap();
    let source = mods.join("A/mod.ini");
    std::fs::write(&source, b"source").unwrap();
    let output = root.join("User/VersionViews/XXMI/3.4/mod.ini");
    std::fs::write(&output, b"prepared").unwrap();
    let game34 = root.join("Game34/YuanShen.exe");
    let game47 = root.join("Game47/GenshinImpact.exe");
    let mut cache = Cache::new();
    let store = |cache: &mut Cache<u32>, game: &Path| {
      let before = input_stamp(&root, game, &mods).unwrap();
      cache.store(&root, game, &mods, &before, 7);
    };
    store(&mut cache, &game34);
    assert_eq!(cache.get(&root, &game34, &mods), Some(7));
    assert_eq!(cache.get(&root, &game47, &mods), None);
    assert_eq!(cache.get(&root, &game34, &root.join("OtherMods")), None);
    std::fs::create_dir_all(mods.join("DISABLED_GIMI_Config")).unwrap();
    std::fs::write(mods.join("DISABLED_GIMI_Config/cache.ini"), b"generated").unwrap();
    assert_eq!(cache.get(&root, &game34, &mods), Some(7));
    let before = input_stamp(&root, &game34, &mods).unwrap();
    std::fs::write(&source, b"edited").unwrap();
    assert_eq!(cache.get(&root, &game34, &mods), None);
    cache.store(&root, &game34, &mods, &before, 9);
    assert_eq!(cache.get(&root, &game34, &mods), None);
    store(&mut cache, &game34);
    std::fs::remove_file(&output).unwrap();
    assert_eq!(cache.get(&root, &game34, &mods), None);
    let buffers = mods.join("DISABLED_GIMI_Config/Buffers/3.4");
    std::fs::create_dir_all(&buffers).unwrap();
    std::fs::write(buffers.join("blend.buf"), b"remapped").unwrap();
    store(&mut cache, &game34);
    std::fs::remove_file(buffers.join("blend.buf")).unwrap();
    assert_eq!(cache.get(&root, &game34, &mods), None);
    store(&mut cache, &game34);
    std::fs::create_dir_all(root.join("User/ModState/XXMI")).unwrap();
    std::fs::write(root.join("User/ModState/XXMI/3.4.ini"), b"$swapvar=2").unwrap();
    assert_eq!(cache.get(&root, &game34, &mods), None);
    store(&mut cache, &game47);
    assert_eq!(cache.get(&root, &game34, &mods), None);
    cache.clear();
    assert_eq!(cache.get(&root, &game47, &mods), None);
    std::fs::remove_dir_all(root).unwrap();
  }
}
