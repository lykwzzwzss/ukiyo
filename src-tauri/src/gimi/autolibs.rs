use crate::gimi::mods;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct Plan {
  pub version: String,
  pub files: Vec<PathBuf>,
  pub ignored: Vec<PathBuf>,
  pub texfx: bool,
  pub offset: bool,
  pub land: String,
  pub shader_key: String,
}

fn active_text(text: &str) -> String {
  text
    .replace("\r", "")
    .split('\n')
    .filter(|line| !line.trim_start().starts_with(';'))
    .map(|line| line.split(';').next().unwrap_or(""))
    .collect::<Vec<&str>>()
    .join("\n")
}

pub fn owned(path: &Path) -> bool {
  let text = match std::fs::read(path) {
    Ok(bytes) => match super::storage::decode_ini(&bytes) { Ok(text) => active_text(&text), Err(_) => return false },
    Err(_) => return false,
  };
  let namespace = text.lines().find_map(|line| {
    let trimmed = line.trim_start();
    if trimmed.starts_with(';') {
      return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("namespace") {
      trimmed
        .split('=')
        .nth(1)
        .map(|value| value.split(';').next().unwrap_or("").trim().to_string())
    } else {
      None
    }
  });
  if let Some(namespace) = namespace {
    let value = namespace.trim().to_ascii_lowercase();
    if value.starts_with("global\\orfix")
      || value.starts_with("global\\offset")
      || value.starts_with("global\\tracking")
      || value.starts_with("global\\region")
      || value.starts_with("global\\healthbar")
      || value.starts_with("global\\submerged")
    {
      return true;
    }
    if value == "texfx"
      && (path.to_string_lossy().to_ascii_lowercase().contains("\\texfx\\")
        || text.contains("[CommandListSetFlags]")
        || text.contains("[CommandListTransparency.0]")
        || text.contains("[CommandListApplyTransparencyOverlay]"))
    {
      return true;
    }
  }
  let file_name = path.file_name().and_then(|value| value.to_str()).unwrap_or("").to_ascii_lowercase();
  let parent = path
    .parent()
    .and_then(|value| value.file_name())
    .and_then(|value| value.to_str())
    .unwrap_or("")
    .to_ascii_lowercase();
  if path.to_string_lossy().to_ascii_lowercase().contains("\\texfx\\") && file_name == "disabled_includes.ini" {
    return true;
  }
  if file_name == "charactershaders.ini" && text.matches("[ShaderOverrideCharacter").count() >= 8 {
    return true;
  }
  if file_name == "underwater.ini" && text.contains("[ShaderOverrideUnderwaterEffect1]") {
    return true;
  }
  parent == "removetransparencyfilter" && (file_name == "charactershaders.ini" || file_name == "underwater.ini")
}

fn collect_ini(directory: &Path, files: &mut Vec<PathBuf>, depth: usize) -> Result<(), String> {
  if depth > 128 { return Err("模组目录层级过深".into()); }
  let entries = std::fs::read_dir(directory).map_err(|error| format!("无法扫描 {}：{}", directory.display(), error))?;
  let mut names: Vec<_> = entries.collect::<Result<_, _>>().map_err(|error| error.to_string())?;
  names.sort_by_key(|entry| entry.file_name());
  for entry in names {
    let path = entry.path();
    if mods::is_link(&path) { continue; }
    let name = entry.file_name().to_string_lossy().into_owned();
    if name.to_ascii_lowercase().starts_with("disabled") {
      continue;
    }
    if path.is_dir() {
      collect_ini(&path, files, depth + 1)?;
    } else if path.extension().and_then(|value| value.to_str()).map(|value| value.eq_ignore_ascii_case("ini")).unwrap_or(false) {
      files.push(path);
    }
  }
  Ok(())
}

pub fn analyze(mods: &Path, game: &Path) -> Result<Plan, String> {
  let mut plan = Plan {
    version: mods::detect_version(game).unwrap_or_default(),
    ..Default::default()
  };
  if !mods.is_dir() {
    return Ok(plan);
  }
  let mut files = Vec::new();
  collect_ini(mods, &mut files, 0)?;
  files.sort();
  for file in files {
    if owned(&file) {
      plan.ignored.push(file);
      continue;
    }
    {
      let text = active_text(&super::storage::read_ini(&file)?);
      if text.to_ascii_lowercase().contains("\\texfx\\") {
        plan.texfx = true;
      }
      if text.to_ascii_lowercase().contains("\\global\\offset\\") {
        plan.offset = true;
      }
    }
    plan.files.push(file);
  }
  plan.texfx |= plan.version == "3.4";
  plan.land = if plan.texfx {
    "TexFX".to_string()
  } else if plan.version == "4.7" {
    "legacy46".to_string()
  } else {
    "none".to_string()
  };
  plan.shader_key = format!(
    "{}-{}{}",
    if plan.version == "4.7" { "47" } else { "34" },
    plan.land,
    if plan.offset { "-offset" } else { "" }
  );
  Ok(plan)
}

pub fn recalculate(plan: &mut Plan, effective_texts: &[(PathBuf, String)]) {
  plan.texfx = plan.version == "3.4";
  plan.offset = false;
  for (_, text) in effective_texts {
    let lower = text.to_ascii_lowercase();
    if lower.contains("\\texfx\\") {
      plan.texfx = true;
    }
    if lower.contains("\\global\\offset\\") {
      plan.offset = true;
    }
  }
  plan.land = if plan.texfx {
    "TexFX".to_string()
  } else if plan.version == "4.7" {
    "legacy46".to_string()
  } else {
    "none".to_string()
  };
  plan.shader_key = format!(
    "{}-{}{}",
    if plan.version == "4.7" { "47" } else { "34" },
    plan.land,
    if plan.offset { "-offset" } else { "" }
  );
}

pub fn includes(plan: &Plan) -> String {
  let mut text = String::from("; Managed compatibility libraries.\n");
  let mut add = |name: &str, path: &str| {
    text.push_str(&format!("[Include {}]\ninclude = ..\\Modules\\{}\n\n", name, path));
  };
  add("ORFix", "ORFix.ini");
  if plan.offset {
    add("Offset", "Offset.ini");
  }
  add("HealthBar", "Auto\\HealthBar.ini");
  if !plan.version.is_empty() {
    let version = if plan.version == "4.7" { "47" } else { "34" };
    add("Region", &format!("Auto\\{}\\Region.ini", version));
    add("Tracking", &format!("Auto\\{}\\Tracking.ini", version));
    add("Water", &format!("Auto\\{}\\Water.ini", version));
    if plan.texfx {
      add("TexFX", "TexFx\\DISABLED_Includes.ini");
    } else if plan.land == "legacy46" {
      add("Land", "Auto\\47\\Land.ini");
    }
  }
  text
}

pub fn shaders(root: &Path, core: &str, plan: &Plan) -> Result<String, String> {
  let directory = root.join("User/RuntimeShaders").join(format!("{}-{}", core, plan.shader_key));
  let relative = format!("..\\..\\User\\RuntimeShaders\\{}", directory.file_name().unwrap_or_default().to_string_lossy());
  let source = root.join("Profiles").join(core).join(if plan.offset { "ShaderFixesWithOffset" } else { "ShaderFixes" });
  let raw = std::fs::read_to_string(root.join("Modules/Auto/land-shader-hashes.json")).map_err(|error| error.to_string())?;
  let owned: Vec<String> = serde_json::from_str(&raw).map_err(|error| error.to_string())?;
  let owned: std::collections::HashSet<_> = owned.into_iter().map(|value| value.to_ascii_lowercase()).collect();
  let mut selected = std::collections::BTreeMap::new();
  for entry in std::fs::read_dir(&source).map_err(|error| format!("缺少着色器目录 {}：{}", source.display(), error))? {
    let entry = entry.map_err(|error| error.to_string())?;
    if !entry.file_type().map_err(|error| error.to_string())?.is_file() { continue; }
    let name = entry.file_name().to_string_lossy().into_owned();
    let lower = name.to_ascii_lowercase();
    if lower.ends_with("-ps_replace.txt") && owned.contains(lower.split('-').next().unwrap_or("")) { continue; }
    selected.insert(name, entry.path());
  }
  if plan.land == "legacy46" {
    for entry in std::fs::read_dir(root.join("Modules/Auto/47/PixelShaders")).map_err(|error| error.to_string())? {
      let entry = entry.map_err(|error| error.to_string())?;
      if entry.file_type().map_err(|error| error.to_string())?.is_file() { selected.insert(entry.file_name().to_string_lossy().into_owned(), entry.path()); }
    }
  }
  let mut fingerprint = String::new();
  for (name, path) in &selected {
    let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
    fingerprint.push_str(&format!("{}:{}:{:?}\n", name, metadata.len(), metadata.modified().ok()));
  }
  let revision = super::versioned::digest(fingerprint.as_bytes());
  let marker = directory.join(".revision");
  if std::fs::read_to_string(&marker).ok().as_deref() == Some(&revision)
    && selected.keys().all(|name| directory.join(name).is_file()) { return Ok(relative); }
  std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
  for (name, path) in &selected {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    super::storage::write_changed(&directory.join(name), bytes)?;
  }
  for entry in std::fs::read_dir(&directory).map_err(|error| error.to_string())? {
    let entry = entry.map_err(|error| error.to_string())?;
    if entry.file_type().map_err(|error| error.to_string())?.is_file()
      && entry.file_name() != ".revision"
      && !selected.contains_key(&entry.file_name().to_string_lossy().into_owned()) {
      std::fs::remove_file(entry.path()).map_err(|error| error.to_string())?;
    }
  }
  super::storage::write_changed(&marker, revision)?;
  Ok(relative)
}

fn clean(path: &Path) -> PathBuf {
  let text = path.to_string_lossy();
  match text.strip_prefix("\\\\?\\") {
    Some(rest) => PathBuf::from(rest),
    None => path.to_path_buf(),
  }
}

pub fn relative_from(from_dir: &Path, to: &Path) -> String {
  let from_dir = clean(from_dir);
  let to = clean(to);
  let from = from_dir.components().collect::<Vec<_>>();
  let target = to.components().collect::<Vec<_>>();
  if from.first() != target.first() {
    return to.to_string_lossy().into_owned();
  }
  let mut common = 0;
  while common < from.len() && common < target.len() && from[common] == target[common] {
    common += 1;
  }
  let mut parts: Vec<String> = Vec::new();
  for _ in common..from.len() {
    parts.push("..".to_string());
  }
  for component in &target[common..] {
    parts.push(component.as_os_str().to_string_lossy().into_owned());
  }
  parts.join("\\")
}

pub fn native_index(
  root: &Path,
  core: &str,
  mods: &Path,
  plan: &Plan,
  prepared: &crate::gimi::versioned::Prepared,
  rules: &crate::gimi::rules::Rules,
) -> Result<String, String> {
  let version = if plan.version.is_empty() { "Unknown" } else { plan.version.as_str() };
  let view_dir = root.join("User").join("VersionViews").join(core).join(version);
  std::fs::create_dir_all(&view_dir).map_err(|error| error.to_string())?;
  let mut planned_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
  for file in &prepared.files {
    planned_ids.insert(crate::gimi::versioned::digest(file.to_string_lossy().to_lowercase().as_bytes())[..24].to_string());
  }
  if let Ok(entries) = std::fs::read_dir(&view_dir) {
    for entry in entries.flatten() {
      let name = entry.file_name().to_string_lossy().into_owned();
      if name.ends_with(".ini") && !planned_ids.contains(name.trim_end_matches(".ini")) {
        let _ = std::fs::remove_file(entry.path());
      }
    }
  }
  let mut catalog = String::from("; One selected hash configuration per original INI.\n");
  let switching = super::switching::build(prepared, rules);
  super::storage::write_changed(&root.join("User/mod-switching.json"), serde_json::to_string_pretty(&switching).map_err(|error| error.to_string())?)?;
  if !switching.families.is_empty() {
    super::storage::write_changed(&root.join("User/mod-switching.ini"), switching.manager())?;
    catalog.push_str("[Include UkiyoModSwitch]\ninclude = ..\\..\\User\\mod-switching.ini\n\n");
  }

  for file in &prepared.files {
    let id = crate::gimi::versioned::digest(file.to_string_lossy().to_lowercase().as_bytes())[..24].to_string();
    let view_path = view_dir.join(format!("{}.ini", id));
    let source_text = prepared
      .converted
      .get(file)
      .map(|item| item.conversion.text.clone())
      .unwrap_or_else(|| std::fs::read_to_string(file).unwrap_or_default());
    let mut text = crate::gimi::versioned::effective(&source_text);
    if super::shared::namespace_of(&text).is_none() {
      let relative = file.strip_prefix(mods).unwrap_or(file).to_string_lossy().replace('/', "\\");
      let namespace = format!("..\\..\\Mods\\{}", relative);
      text = format!("namespace = {}\n{}", namespace, text);
    }
    text = text
      .lines()
      .map(|line| {
        let lower = line.trim_start().to_ascii_lowercase();
        if lower.starts_with("include_recursive") {
          "; Managed catalog selects enabled source INIs".to_string()
        } else {
          line.to_string()
        }
      })
      .collect::<Vec<String>>()
      .join("\n");

    let mut lines: Vec<String> = Vec::new();
    for line in text.split('\n') {
      let trimmed = line.trim_start();
      let lower = trimmed.to_ascii_lowercase();
      let is_key = lower.starts_with("filename")
        || lower.starts_with("include")
        || lower.starts_with("vs")
        || lower.starts_with("ps")
        || lower.starts_with("hs")
        || lower.starts_with("ds")
        || lower.starts_with("gs")
        || lower.starts_with("cs");
      if !is_key || trimmed.starts_with(';') {
        lines.push(line.to_string());
        continue;
      }
      let Some(equals) = trimmed.find('=') else {
        lines.push(line.to_string());
        continue;
      };
      let key = trimmed[..equals].trim().to_ascii_lowercase();
      if key != "filename" && key != "include" && !["vs", "ps", "hs", "ds", "gs", "cs"].contains(&key.as_str()) {
        lines.push(line.to_string());
        continue;
      }
      let value = trimmed[equals + 1..].split(';').next().unwrap_or("").trim().trim_matches('"');
      if value.is_empty() {
        lines.push(line.to_string());
        continue;
      }
      let resolved = if Path::new(value).is_absolute() {
        PathBuf::from(value)
      } else {
        file.parent().unwrap_or(mods).join(value)
      };
      if key == "include" {
        let canonical = std::fs::canonicalize(&resolved).ok();
        if prepared.converted.keys().any(|source| {
          source == &resolved || canonical.as_ref().map_or(false, |target| std::fs::canonicalize(source).ok().as_ref() == Some(target))
        }) || owned(&resolved) {
          lines.push("; Managed catalog includes this file independently".to_string());
          continue;
        }
      }
      if resolved.exists() {
        let relative = relative_from(&view_dir, &resolved);
        let indent = &line[..line.len() - trimmed.len()];
        lines.push(format!("{}{} = {}", indent, trimmed[..equals].trim(), relative));
      } else {
        lines.push(line.to_string());
      }
    }
    super::storage::write_changed(&view_path, switching.apply(file, &lines.join("\n")))?;
    catalog.push_str(&format!(
      "[Include Managed{}]\ninclude = ..\\..\\User\\VersionViews\\{}\\{}\\{}.ini\n\n",
      catalog.len(),
      core,
      version,
      id
    ));
  }
  let catalog_path = root.join("Profiles").join(core).join("mod-catalog.ini");
  super::storage::write_changed(&catalog_path, catalog)?;
  Ok("mod-catalog.ini".to_string())
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn shader_cache_is_reused_and_updates_when_sources_change() {
    let root = std::env::temp_dir().join(format!("ukiyo-shader-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let source = root.join("Profiles/XXMI/ShaderFixes");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(root.join("Modules/Auto")).unwrap();
    std::fs::write(root.join("Modules/Auto/land-shader-hashes.json"), "[]").unwrap();
    std::fs::write(source.join("test.hlsl"), "first").unwrap();
    let plan = Plan { shader_key: "test".into(), ..Default::default() };
    shaders(&root, "XXMI", &plan).unwrap();
    let target = root.join("User/RuntimeShaders/XXMI-test/test.hlsl");
    let before = std::fs::metadata(&target).unwrap().modified().unwrap();
    shaders(&root, "XXMI", &plan).unwrap();
    assert_eq!(std::fs::metadata(&target).unwrap().modified().unwrap(), before);
    std::fs::write(source.join("test.hlsl"), "second changed").unwrap();
    shaders(&root, "XXMI", &plan).unwrap();
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "second changed");
    let _ = std::fs::remove_dir_all(root);
  }
}
