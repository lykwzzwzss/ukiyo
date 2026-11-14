//! Only remove files installed by this launcher; preserve and restore prior contents.
use serde::{Deserialize, Serialize};
use std::{fs, io::{Read, Write}, path::{Path, PathBuf}, sync::Mutex};

static LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize)]
struct Entry {
  destination: PathBuf,
  backup: Option<PathBuf>,
  fingerprint: u64,
}

fn fingerprint(path: &Path) -> Result<u64, String> {
  let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
  let mut hash = 0xcbf29ce484222325u64;
  let mut buffer = [0u8; 16384];
  loop {
    let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
    if count == 0 { return Ok(hash); }
    for byte in &buffer[..count] { hash = (hash ^ *byte as u64).wrapping_mul(0x100000001b3); }
  }
}

fn read_entries(root: &Path) -> Result<Vec<Entry>, String> {
  match fs::read(root.join("managed-patches.json")) {
    Ok(raw) => serde_json::from_slice(&raw).map_err(|e| e.to_string()),
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
    Err(e) => Err(e.to_string()),
  }
}

fn save_entries(root: &Path, entries: &[Entry]) -> Result<(), String> {
  fs::create_dir_all(root).map_err(|e| e.to_string())?;
  let temporary = root.join("managed-patches.json.tmp");
  let mut file = fs::File::create(&temporary).map_err(|e| e.to_string())?;
  file.write_all(&serde_json::to_vec(entries).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
  file.sync_all().map_err(|e| e.to_string())?;
  drop(file);
  fs::rename(temporary, root.join("managed-patches.json")).map_err(|e| e.to_string())
}

fn install_at(root: &Path, source: &Path, destination: &Path) -> Result<(), String> {
  let expected = fingerprint(source)?;
  let destination = fs::canonicalize(destination.parent().ok_or("Missing game directory")?)
    .map_err(|e| e.to_string())?.join(destination.file_name().ok_or("Missing patch filename")?);
  let mut entries = read_entries(root)?;
  if let Some(entry) = entries.iter().find(|e| e.destination == destination) {
    if fingerprint(&destination)? != entry.fingerprint { return Err("Managed patch was modified externally".into()); }
    if entry.fingerprint == expected { return Ok(()); }
    return Err("Remove the previous managed patch before installing another version".into());
  }
  if destination.exists() && fingerprint(&destination)? == expected {
    // A manually installed matching patch is usable, but is not ours to delete.
    return Ok(());
  }
  let temporary = temporary_path(&destination);
  fs::copy(source, &temporary).map_err(|e| e.to_string())?;
  fs::create_dir_all(root.join("patch-backups")).map_err(|e| e.to_string())?;
  let backup = if destination.exists() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
    let backup = root.join("patch-backups").join(format!("{}-{}", std::process::id(), nonce));
    fs::copy(&destination, &backup).map_err(|e| e.to_string())?;
    Some(backup)
  } else { None };
  entries.push(Entry { destination: destination.clone(), backup, fingerprint: expected });
  // Persist ownership before replacement, so a crash cannot leave an untracked patch.
  save_entries(root, &entries)?;
  if let Err(error) = fs::rename(&temporary, &destination) {
    let failed = entries.pop().unwrap();
    save_entries(root, &entries)?;
    if let Some(backup) = failed.backup { let _ = fs::remove_file(backup); }
    let _ = fs::remove_file(temporary);
    return Err(error.to_string());
  }
  Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
  let mut name = path.as_os_str().to_os_string();
  name.push(".ukiyo.tmp");
  PathBuf::from(name)
}

fn cleanup_at(root: &Path) -> Result<(), String> { cleanup_selected_at(root, None) }

fn cleanup_selected_at(root: &Path, selected: Option<&std::collections::HashSet<PathBuf>>) -> Result<(), String> {
  let mut retained = Vec::new();
  let mut errors = Vec::new();
  let mut obsolete_backups = Vec::new();
  let entries = read_entries(root)?;
  if entries.is_empty() { return Ok(()); }
  for entry in entries {
    if selected.map_or(false, |paths| !paths.contains(&entry.destination)) {
      retained.push(entry);
      continue;
    }
    let result = (|| -> Result<(), String> {
      if entry.destination.exists() && fingerprint(&entry.destination)? != entry.fingerprint {
        if let Some(backup) = &entry.backup {
          if fingerprint(backup)? == fingerprint(&entry.destination)? { return Ok(()); }
        }
        return Err(format!("Preserved externally modified file: {}", entry.destination.display()));
      }
      if let Some(backup) = &entry.backup {
        let temporary = temporary_path(&entry.destination);
        fs::copy(backup, &temporary).map_err(|e| e.to_string())?;
        fs::rename(temporary, &entry.destination).map_err(|e| e.to_string())?;
      } else if entry.destination.exists() {
        fs::remove_file(&entry.destination).map_err(|e| e.to_string())?;
      }
      Ok(())
    })();
    if let Err(error) = result { errors.push(error); retained.push(entry); }
    else if let Some(backup) = entry.backup { obsolete_backups.push(backup); }
  }
  save_entries(root, &retained)?;
  for backup in obsolete_backups { if let Err(e) = fs::remove_file(backup) { eprintln!("Could not remove obsolete patch backup: {}", e); } }
  if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
}

fn install_bundle_at(root: &Path, files: &[(PathBuf, PathBuf)]) -> Result<(), String> {
  let before: std::collections::HashSet<_> = read_entries(root)?.into_iter().map(|entry| entry.destination).collect();
  for (source, destination) in files {
    if let Err(error) = install_at(root, source, destination) {
      let added = read_entries(root)?.into_iter().filter(|entry| !before.contains(&entry.destination)).map(|entry| entry.destination).collect();
      if let Err(rollback) = cleanup_selected_at(root, Some(&added)) {
        return Err(format!("{}; rollback failed: {}", error, rollback));
      }
      return Err(error);
    }
  }
  Ok(())
}

pub fn install_payloads(files: &[(PathBuf, Vec<u8>)]) -> Result<(), String> {
  let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
  let root = tauri::api::path::data_dir().ok_or("Missing data directory")?.join("cultivation");
  let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
  let staging = root.join("patch-staging").join(format!("{}-{}", std::process::id(), nonce));
  fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
  let mut staged = Vec::new();
  let result = (|| {
    for (index, (destination, bytes)) in files.iter().enumerate() {
      let source = staging.join(format!("{}.bin", index));
      staged.push((source.clone(), destination.clone()));
      fs::write(source, bytes).map_err(|e| e.to_string())?;
    }
    install_bundle_at(&root, &staged)
  })();
  for (source, _) in staged { let _ = fs::remove_file(source); }
  let _ = fs::remove_dir(staging);
  result
}

pub fn install(source: &Path, destination: &Path) -> bool {
  let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
  let result = tauri::api::path::data_dir().ok_or("Missing data directory".to_string())
    .and_then(|root| install_at(&root.join("cultivation"), source, destination));
  if let Err(e) = &result { eprintln!("Patch install failed: {}", e); }
  result.is_ok()
}

pub fn cleanup() -> bool {
  let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
  let result = tauri::api::path::data_dir().ok_or("Missing data directory".to_string())
    .and_then(|root| cleanup_at(&root.join("cultivation")));
  if let Err(e) = &result { eprintln!("Patch cleanup failed: {}", e); }
  result.is_ok()
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn ownership_and_restore() {
    let root = std::env::temp_dir().join(format!("cultivation-patches-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let source = root.join("patch.dll");
    let destination = root.join("version.dll");
    fs::write(&source, b"patch").unwrap();
    fs::write(&destination, b"original").unwrap();
    install_at(&root, &source, &destination).unwrap();
    cleanup_at(&root).unwrap();
    assert_eq!(fs::read(&destination).unwrap(), b"original");
    install_at(&root, &source, &destination).unwrap();
    fs::write(&destination, b"external change").unwrap();
    assert!(cleanup_at(&root).is_err());
    assert_eq!(fs::read(&destination).unwrap(), b"external change");
    // The test owns this uniquely named temporary directory.
    fs::remove_dir_all(root).unwrap();
  }
  #[test]
  fn two_file_bundle_restores_both_and_rolls_back_partial_install() {
    let root = std::env::temp_dir().join(format!("ukiyo-34-bundle-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let dll_source = root.join("source.dll");
    let ini_source = root.join("source.ini");
    let dll = root.join("mhypbase.dll");
    let ini = root.join("mhypbase.ini");
    fs::write(&dll_source, b"new dll").unwrap();
    fs::write(&ini_source, b"new config").unwrap();
    fs::write(&dll, b"original dll").unwrap();
    fs::write(&ini, b"original config").unwrap();
    let files = [(dll_source.clone(), dll.clone()), (ini_source.clone(), ini.clone())];
    install_bundle_at(&root, &files).unwrap();
    assert_eq!(fs::read(&ini).unwrap(), b"new config");
    cleanup_at(&root).unwrap();
    assert_eq!(fs::read(&dll).unwrap(), b"original dll");
    assert_eq!(fs::read(&ini).unwrap(), b"original config");
    let invalid = [(dll_source, dll.clone()), (ini_source, root.join("missing").join("mhypbase.ini"))];
    assert!(install_bundle_at(&root, &invalid).is_err());
    assert_eq!(fs::read(&dll).unwrap(), b"original dll");
    assert!(read_entries(&root).unwrap().is_empty());
    // This uniquely named temporary directory belongs to the test.
    fs::remove_dir_all(root).unwrap();
  }

}
