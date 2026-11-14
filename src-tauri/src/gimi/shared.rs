use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Value {
  #[serde(default)] pub text: String,
  #[serde(default)] pub ticks: u64,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Store {
  #[serde(default)] pub schema: u32,
  #[serde(default)] pub aliases: HashMap<String, String>,
  #[serde(default)] pub values: HashMap<String, Value>,
  #[serde(default)] pub pending: HashMap<String, Value>,
  #[serde(default)] pub seen: HashMap<String, String>,
}

fn store_path(root: &Path) -> PathBuf {
  root.join("User").join("shared-mod-state.json")
}

fn load(root: &Path) -> Result<Store, String> {
  let mut store: Store = match std::fs::read_to_string(store_path(root)) {
    Ok(raw) => serde_json::from_str(&raw).map_err(|error| format!("共享模组状态文件损坏，请保留文件后检查：{}", error))?,
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Store::default(),
    Err(error) => return Err(error.to_string()),
  };
  if store.schema > 1 { return Err("共享模组状态版本不受支持".into()); }
  if store.schema == 0 {
    store.schema = 1;
  }
  store.aliases = store.aliases.into_iter().map(|(name, value)| (name.to_lowercase(), value)).collect();
  Ok(store)
}

fn save(root: &Path, store: &Store) -> Result<(), String> {
  let json = serde_json::to_vec_pretty(store).map_err(|error| error.to_string())?;
  super::storage::write_changed(&store_path(root), json)
}

pub(super) fn namespace_of(text: &str) -> Option<String> {
  text.lines().find_map(|line| {
    let trimmed = line.trim_start();
    if trimmed.starts_with(';') {
      return None;
    }
    let (name, value) = trimmed.split_once('=')?;
    if name.trim().eq_ignore_ascii_case("namespace") { Some(value.split(';').next()?.trim().to_string()) } else { None }
  })
}

fn persist_variables(text: &str) -> Vec<String> {
  let mut variables = Vec::new();
  for line in text.lines() {
    let trimmed = line.trim_start();
    if trimmed.starts_with(';') {
      continue;
    }
    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with("global") {
      continue;
    }
    let mut parts = trimmed.split_whitespace();
    if !parts.next().map_or(false, |word| word.eq_ignore_ascii_case("global")) { continue; }
    let Some(persist) = parts.next() else { continue };
    if !persist.eq_ignore_ascii_case("persist") {
      continue;
    }
    let Some(variable) = parts.next() else { continue };
    if let Some(name) = variable.strip_prefix('$') {
      if !name.is_empty() {
        variables.push(name.split(['=', ';']).next().unwrap_or(name).to_string());
      }
    }
  }
  variables
}

fn key(namespace: &str, variable: &str) -> String {
  if namespace.is_empty() {
    "$".to_string() + variable
  } else {
    "$\\".to_string() + namespace.trim_matches('\\') + "\\" + variable
  }
}

// Import namespaces saved by the retired V7 core; no V7 code is loaded.
fn legacy_namespace(relative: &str) -> String {
  let relative = relative.replace('/', "\\");
  if relative.is_ascii() { format!("..\\..\\Mods\\{}", relative) }
  else { format!("ukiyo_{}", &super::versioned::digest(relative.to_lowercase().as_bytes())[..16]) }
}

fn collect_values(root: &Path, store: &mut Store) -> Result<HashMap<String, Value>, String> {
  let mut values: HashMap<String, Value> = HashMap::new();
  let mut files: Vec<PathBuf> = Vec::new();
  for core in ["V7", "XXMI"] {
  let state = root.join("User").join("ModState").join(core);
  if let Ok(entries) = std::fs::read_dir(&state) {
    for entry in entries.flatten() {
      if entry.path().extension().and_then(|value| value.to_str()).map(|value| value.eq_ignore_ascii_case("ini")).unwrap_or(false) {
        files.push(entry.path());
      }
    }
  }
  let legacy = root.join("Profiles").join(core).join("d3dx_user.ini");
  if legacy.is_file() {
    files.push(legacy);
  }
  }
  files.sort_by_key(|path| std::fs::metadata(path).and_then(|meta| meta.modified()).ok());
  for file in files {
    let bytes = std::fs::read(&file).map_err(|error| error.to_string())?;
    let hash = super::versioned::digest(&bytes);
    let file_key = file.to_string_lossy().into_owned();
    if store.seen.get(&file_key) == Some(&hash) { continue; }
    let ticks = std::fs::metadata(&file).and_then(|meta| meta.modified()).ok()
      .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
      .map(|duration| 621355968000000000 + (duration.as_nanos() / 100) as u64)
      .unwrap_or(0);
    for line in super::storage::decode_ini(&bytes)?.lines() {
      let trimmed = line.trim_start();
      if !trimmed.starts_with('$') {
        continue;
      }
      let Some(equals) = trimmed.find('=') else { continue };
      let name = trimmed[..equals].trim().to_lowercase();
      let value = trimmed[equals + 1..].split(';').next().unwrap_or("").trim();
      if value.parse::<f64>().map_or(false, f64::is_finite) {
        let entry = Value { text: value.to_string(), ticks };
        match values.get(&name) {
          Some(existing) if existing.ticks > ticks => {}
          _ => {
            values.insert(name, entry);
          }
        }
      }
    }
    store.seen.insert(file_key, hash);
  }
  Ok(values)
}

fn allowed_variables(path: &Path, visited: &mut std::collections::HashSet<PathBuf>, names: &mut std::collections::BTreeSet<String>) -> Result<(), String> {
  let path = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
  if !visited.insert(path.clone()) { return Ok(()); }
  let text = super::storage::read_ini(&path)?;
  let namespace = namespace_of(&text).unwrap_or_default();
  for variable in persist_variables(&text) { names.insert(key(&namespace, &variable)); }
  for line in text.lines() {
    let line = line.split(';').next().unwrap_or("").trim();
    let Some((name, value)) = line.split_once('=') else { continue };
    if !name.trim().eq_ignore_ascii_case("include") { continue; }
    let include = path.parent().unwrap().join(value.trim().trim_matches('"'));
    if include.is_file() { allowed_variables(&include, visited, names)?; }
  }
  Ok(())
}

pub fn prepare(root: &Path, core: &str, version: &str, files: &[PathBuf], mods_root: &Path) -> Result<(), String> {
  let mut store = load(root)?;
  let mut aliases: HashMap<String, String> = HashMap::new();
  for file in files {
    let text = super::storage::read_ini(file)?;
    let namespace = namespace_of(&text).unwrap_or_else(|| {
      let relative = file.strip_prefix(mods_root).unwrap_or(file).to_string_lossy().replace('/', "\\");
      format!("..\\..\\Mods\\{}", relative)
    });
    let logical_root = file.to_string_lossy().to_lowercase();
    let relative = file.strip_prefix(mods_root).unwrap_or(file).to_string_lossy().to_string();
    for variable in persist_variables(&text) {
      let original = key(&namespace, &variable);
      let generated = key(&legacy_namespace(&relative), &variable);
      let logical = format!("{}\\{}", logical_root, variable);
      aliases.insert(original.to_lowercase(), logical.clone());
      aliases.insert(generated.to_lowercase(), logical.clone());
    }
  }
  store.aliases.extend(aliases);
  let collected = collect_values(root, &mut store)?;
  for (name, value) in collected {
    if let Some(logical) = store.aliases.get(&name) {
      if store.values.get(logical).map_or(true, |entry| value.ticks > entry.ticks) {
        store.values.insert(logical.clone(), value);
      }
    } else if store.pending.get(&name).map_or(true, |entry| value.ticks > entry.ticks) {
      store.pending.insert(name, value);
    }
  }
  let tag = if version.is_empty() { "Unknown" } else { version };
  let mut text = String::from("; Shared persistent Mod state. Generated for the currently loaded variables.\n[Constants]\n");
  let mut names = std::collections::BTreeSet::new();
  allowed_variables(&root.join("Profiles").join(core).join("d3dx.ini"), &mut std::collections::HashSet::new(), &mut names)?;
  for name in &names { let name = name.to_lowercase(); store.aliases.entry(name.clone()).or_insert(name); }
  // Promote unregistered variables once their Mod is enabled again. Also import
  // values saved under raw names by the first Rust integration.
  for (name, value) in store.values.clone() {
    if name.starts_with('$') { store.pending.entry(name.to_lowercase()).or_insert(value); }
  }
  let pending = std::mem::take(&mut store.pending);
  for (name, value) in pending {
    if let Some(logical) = store.aliases.get(&name.to_lowercase()) {
      if store.values.get(logical).map_or(true, |entry| value.ticks > entry.ticks) { store.values.insert(logical.clone(), value); }
    } else { store.pending.insert(name, value); }
  }
  for name in names {
    let normalized = name.to_lowercase();
    let logical = store.aliases.get(&normalized).unwrap_or(&normalized);
    if let Some(value) = store.values.get(logical) {
      text.push_str(&name);
      text.push_str(" = ");
      text.push_str(&value.text);
      text.push('\n');
    }
  }
  let path = root.join("User").join("ModState").join(core).join(format!("{}.ini", tag));
  if let Some(parent) = path.parent() {
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
  }
  let text = text.replace('\n', "\r\n");
  super::storage::write_changed(&path, &text)?;
  store.seen.insert(path.to_string_lossy().into_owned(), super::versioned::digest(text.as_bytes()));
  save(root, &store)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_persistent_variables() {
    let text = "[Constants]\nnamespace = ..\\..\\Mods\\Ayaka\nglobal persist $outfit = 0\nglobal persist $color = 1\n";
    assert_eq!(persist_variables(text), vec!["outfit", "color"]);
    assert_eq!(namespace_of(text).as_deref(), Some("..\\..\\Mods\\Ayaka"));
    assert_eq!(key("..\\..\\Mods\\Ayaka", "outfit"), "$\\..\\..\\Mods\\Ayaka\\outfit");
    assert_eq!(persist_variables("global persist $outfit=0"), vec!["outfit"]);
  }

  #[test]
  fn imports_old_core_values_and_does_not_treat_generated_outputs_as_user_edits() {
    let root = std::env::temp_dir().join(format!("ukiyo-shared-state-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mods = root.join("Mods");
    let file = mods.join("角色/mod.ini");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "global persist $outfit=0\n").unwrap();
    let namespace = "..\\..\\Mods\\角色\\mod.ini";
    let profile = root.join("Profiles/XXMI");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("d3dx.ini"), format!("namespace = {}\n[Constants]\nglobal persist $outfit=0\n[Include]\ninclude = builtin.ini\n", namespace)).unwrap();
    std::fs::write(profile.join("builtin.ini"), "[Constants]\nglobal persist $builtin=0\n").unwrap();
    let old = root.join("User/ModState/V7/4.7.ini");
    let current = root.join("User/ModState/XXMI/4.7.ini");
    for path in [&old, &current] { std::fs::create_dir_all(path.parent().unwrap()).unwrap(); }
    let write_at = |path: &Path, text: String, time| {
      std::fs::write(path, text).unwrap();
      std::fs::OpenOptions::new().write(true).open(path).unwrap().set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(time)).unwrap();
    };
    write_at(&old, format!("[Constants]\n{} = 7\n", key(&legacy_namespace("角色\\mod.ini"), "outfit")), 2000);
    write_at(&current, format!("[Constants]\n{} = 1\n$builtin = 4\n", key(namespace, "outfit")), 1000);
    prepare(&root, "XXMI", "4.7", &[file.clone()], &mods).unwrap();
    let text = std::fs::read_to_string(&current).unwrap();
    assert!(text.contains("outfit = 7"));
    assert!(text.contains("$builtin = 4"));
    prepare(&root, "XXMI", "3.4", &[file.clone()], &mods).unwrap();
    write_at(&current, format!("[Constants]\n{} = 9\n$builtin = 4\n", key(namespace, "outfit")), 3000);
    prepare(&root, "XXMI", "3.4", &[file.clone()], &mods).unwrap();
    assert!(std::fs::read_to_string(root.join("User/ModState/XXMI/3.4.ini")).unwrap().contains("outfit = 9"));
    write_at(&old, format!("[Constants]\n{} = 2\n", key(&legacy_namespace("角色\\mod.ini"), "outfit")), 2500);
    prepare(&root, "XXMI", "4.7", &[file], &mods).unwrap();
    assert!(std::fs::read_to_string(&current).unwrap().contains("outfit = 9"));
    std::fs::write(store_path(&root), "{bad").unwrap();
    assert!(prepare(&root, "XXMI", "4.7", &[], &mods).is_err());
    assert_eq!(std::fs::read_to_string(store_path(&root)).unwrap(), "{bad");
    let _ = std::fs::remove_dir_all(root);
  }
}
