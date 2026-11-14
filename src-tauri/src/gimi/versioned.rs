use crate::gimi::ini;
use crate::gimi::migrations::Migrations;
use crate::gimi::rules::Rules;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Conversion {
  pub text: String,
  pub status: String,
  pub reason: String,
  pub character: String,
  pub changes: usize,
  pub blocked: bool,
}

#[derive(Clone, Debug)]
pub struct Converted {
  pub source: PathBuf,
  pub group: String,
  pub original: PathBuf,
  pub variant: PathBuf,
  pub conversion: Conversion,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Report {
  pub target: String,
  pub revision: String,
  pub groups: BTreeMap<String, String>,
  pub changed_files: usize,
  pub blocked_files: usize,
}

pub struct Prepared {
  pub version: String,
  pub files: Vec<PathBuf>,
  pub converted: HashMap<PathBuf, Converted>,
  pub report: Report,
}

pub fn digest(bytes: &[u8]) -> String {
  let mut hasher = Sha256::new();
  hasher.update(bytes);
  let result = hasher.finalize();
  result.iter().map(|value| format!("{:02x}", value)).collect()
}

fn id_for(root: &Path, canonical: &str) -> String {
  let seed = format!("{}\\{}", root.to_string_lossy(), canonical).to_lowercase();
  digest(seed.as_bytes())[..24].to_string()
}

fn canonical_relative(root: &Path, file: &Path) -> String {
  let relative = file.strip_prefix(root).unwrap_or(file);
  let mut parts: Vec<String> = relative
    .components()
    .map(|component| component.as_os_str().to_string_lossy().into_owned())
    .collect();
  if let Some(first) = parts.first_mut() {
    let lower = first.to_ascii_lowercase();
    if lower.starts_with("disabled") {
      let mut index = "disabled".len();
      let bytes = first.as_bytes();
      while index < bytes.len() && (bytes[index] == b'_' || bytes[index] == b'-' || bytes[index] == b' ') {
        index += 1;
      }
      *first = first[index..].to_string();
    }
  }
  parts.join("\\")
}

fn group_of(relative: &str) -> String {
  relative.split('\\').next().unwrap_or(relative).to_string()
}

fn active_commands(body: &str) -> Vec<String> {
  body
    .replace("\r", "")
    .split('\n')
    .map(|line| line.split(';').next().unwrap_or("").trim().to_string())
    .filter(|line| !line.is_empty())
    .collect()
}

fn is_model_section(name: &str) -> bool {
  let lower = name.to_ascii_lowercase();
  ["position", "blend", "texcoord", "vertexlimit", "head", "body", "dress", "ib"]
    .iter()
    .any(|suffix| lower.ends_with(suffix))
}

fn parse_hash_line(line: &str) -> Option<(String, String, String)> {
  let trimmed = line.trim_start();
  if trimmed.starts_with(';') {
    return None;
  }
  let equals = trimmed.find('=')?;
  if !trimmed[..equals].trim().eq_ignore_ascii_case("hash") {
    return None;
  }
  let rest = trimmed[equals + 1..].trim_start();
  let value = rest.split(';').next().unwrap_or("").trim();
  if value.len() != 8 || !value.chars().all(|character| character.is_ascii_hexdigit()) {
    return None;
  }
  let indent = &line[..line.len() - trimmed.len()];
  let after = &rest[value.len()..];
  Some((indent.to_string(), value.to_ascii_lowercase(), after.to_string()))
}

fn known_hashes(text: &str) -> Vec<String> {
  let document = ini::parse(text);
  let mut seen = HashSet::new();
  let mut hashes = Vec::new();
  for section in &document.sections {
    if !section.name.to_ascii_lowercase().starts_with("textureoverride") {
      continue;
    }
    for line in ini::body(&section.raw).split_inclusive('\n') {
      if let Some((_, hash, _)) = parse_hash_line(line) {
        if seen.insert(hash.clone()) {
          hashes.push(hash);
        }
      }
    }
  }
  hashes
}

pub fn convert(text: &str, target: &str, origin: &str, rules: &Rules, migrations: &Migrations) -> Conversion {
  let document = ini::parse(text);
  let sections: Vec<&ini::Section> = document
    .sections
    .iter()
    .filter(|section| section.name.to_ascii_lowercase().starts_with("textureoverride"))
    .collect();
  let known = known_hashes(text);

  let mut scores: HashMap<String, HashSet<String>> = HashMap::new();
  for hash in &known {
    for index in rules.candidates(hash) {
      let rule = rules.rule(*index);
      if Rules::is_anchor(&rule.role) {
        scores.entry(rule.character.clone()).or_default().insert(hash.clone());
      }
    }
  }
  let mut characters: Vec<String> = scores
    .iter()
    .filter(|(_, values)| values.len() >= 2)
    .map(|(character, _)| character.clone())
    .collect();
  characters.sort();
  let character_label = characters.join(",");

  let mut result = Conversion {
    text: text.to_string(),
    status: "原配置".to_string(),
    character: character_label.clone(),
    ..Default::default()
  };

  if origin.eq_ignore_ascii_case(target) {
    result.status = format!("原始 {}", target);
    return result;
  }

  let looks_like_model = sections.iter().any(|section| is_model_section(&section.name));
  if characters.is_empty() {
    result.blocked = looks_like_model && !known.is_empty();
    result.status = if result.blocked { "需要适配".to_string() } else { "原配置".to_string() };
    result.reason = if result.blocked {
      "不足两个独立模型入口，无法可靠识别角色".to_string()
    } else {
      "非角色配置或没有可识别的角色哈希，原样共用".to_string()
    };
    return result;
  }
  if target == "3.4" && characters.iter().any(|character| !rules.has_character34(character)) {
    result.blocked = true;
    result.status = "3.4 无对应角色".to_string();
    result.reason = "目标版本没有原生角色或缺少参考数据".to_string();
    return result;
  }

  let mut failure = String::new();
  let mut output = String::new();
  output.push_str(&document.preamble);
  for section in &document.sections {
    let is_override = section.name.to_ascii_lowercase().starts_with("textureoverride");
    if !is_override {
      output.push_str(&section.raw);
      continue;
    }
    let (header, body_text) = ini::split_header(&section.raw);
    output.push_str(header);
    let body_has_match_first = body_text
      .lines()
      .any(|line| line.trim_start().to_ascii_lowercase().starts_with("match_first_index"));
    let section_is_model = is_model_section(&section.name);
    for line in body_text.split_inclusive('\n') {
      match parse_hash_line(line) {
        None => output.push_str(line),
        Some((indent, hash, after)) => {
          let all = rules.candidates(&hash);
          if all.is_empty() {
            if section_is_model || body_has_match_first {
              failure = "关键模型哈希未覆盖".to_string();
            }
            output.push_str(line);
            continue;
          }
          let candidates: Vec<&crate::gimi::rules::Rule> = all
            .iter()
            .map(|index| rules.rule(*index))
            .filter(|rule| characters.iter().any(|character| character.eq_ignore_ascii_case(&rule.character)))
            .collect();
          if candidates.is_empty() {
            output.push_str(line);
            continue;
          }
          let usable: Vec<&crate::gimi::rules::Rule> = candidates
            .into_iter()
            .filter(|rule| {
              if target == "4.7" {
                rule.target.eq_ignore_ascii_case(&hash)
                  || ((rule.layout_consistent || migrations.can_map(rule)) && rule.source.eq_ignore_ascii_case(&hash))
              } else {
                rule.reference.eq_ignore_ascii_case("3.4-candidate")
                  && (rule.source.eq_ignore_ascii_case(&hash)
                    || ((rule.layout_consistent || migrations.can_map(rule)) && rule.target.eq_ignore_ascii_case(&hash)))
              }
            })
            .collect();
          if usable.is_empty() {
            failure = "模型入口或部件结构存在差异，需要专门规则".to_string();
            output.push_str(line);
            continue;
          }
          let mut destinations: Vec<String> = usable
            .iter()
            .map(|rule| if target == "4.7" { rule.target.clone() } else { rule.source.clone() })
            .collect();
          destinations.sort();
          destinations.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
          if destinations.len() != 1 {
            failure = "同一哈希存在多个目标，未自动选择".to_string();
            output.push_str(line);
            continue;
          }
          if let Some(rest) = body_text.lines().find(|line| line.trim_start().to_ascii_lowercase().starts_with("match_first_index")) {
            if let Some(index_text) = rest.split('=').nth(1) {
              if let Ok(index) = index_text.trim().parse::<u32>() {
                let has_ib = usable.iter().any(|rule| rule.role == "ib");
                let allowed = usable
                  .iter()
                  .any(|rule| rule.role == "ib" && rule.indexes.contains(&index));
                if has_ib && !allowed {
                  failure = "部件索引不在参考范围内".to_string();
                  output.push_str(line);
                  continue;
                }
              }
            }
          }
          let destination = &destinations[0];
          if !destination.eq_ignore_ascii_case(&hash) {
            result.changes += 1;
          }
          let ending = if line.ends_with("\r\n") { "\r\n" } else if line.ends_with('\n') { "\n" } else { "" };
          output.push_str(&indent);
          output.push_str("hash = ");
          output.push_str(destination);
          output.push_str(after.trim_end_matches(['\r', '\n']));
          output.push_str(ending);
        }
      }
    }
  }

  if failure.is_empty() {
    result.text = output;
    result.status = if result.changes > 0 { format!("{} 哈希适配·待实测", target) } else { format!("{} 哈希一致", target) };
    result.reason = "仅改配置入口，未转换模型、骨骼或资源；参考资料尚未逐模组实机验证".to_string();
  } else {
    result.blocked = true;
    result.status = "需要专门适配".to_string();
    result.reason = failure;
    result.changes = 0;
  }
  result
}

fn comment_lines(raw: &str, prefix: &str) -> String {
  let mut output = String::new();
  for line in raw.split_inclusive('\n') {
    if line.trim().is_empty() {
      output.push_str(line);
    } else {
      output.push_str(prefix);
      output.push_str(line);
    }
  }
  output
}

pub fn effective(text: &str) -> String {
  let mut document = ini::parse(text);
  for section in document.sections.iter_mut() {
    let lower = section.name.to_ascii_lowercase();
    let key = if lower == "4.0_updated" {
      Some("updated")
    } else if lower == "mod_4.4" {
      Some("from44")
    } else {
      None
    };
    if let Some(key) = key {
      let commands = active_commands(ini::body(&section.raw));
      let only_marker = commands.iter().all(|line| {
        line.split_once('=').map_or(false, |(name, value)| {
          name.trim().eq_ignore_ascii_case(key) && value.trim().eq_ignore_ascii_case("yes")
        })
      });
      if only_marker {
        // The section header itself is unknown to 3DMigoto, even with an empty
        // body. Comment the complete section, not just its metadata assignment.
        section.raw = comment_lines(&section.raw, "; Migration metadata: ");
      }
    }
  }
  let mut text_out = document.render();

  let mut declared: HashSet<String> = HashSet::new();
  for section in &document.sections {
    if section.name.to_ascii_lowercase().starts_with("resource") {
      declared.insert(section.name.clone());
    }
  }
  let mut restores: Vec<String> = Vec::new();
  for line in text_out.lines() {
    let trimmed = line.trim_start();
    if trimmed.starts_with(';') || trimmed.is_empty() {
      continue;
    }
    let Some(equals) = trimmed.find('=') else { continue };
    let name = trimmed[..equals].trim();
    if !name.to_ascii_lowercase().starts_with("resourceref") {
      continue;
    }
    let value = trimmed[equals + 1..].split(';').next().unwrap_or("").trim();
    if !value.to_ascii_lowercase().starts_with("reference ps-t") {
      continue;
    }
    if declared.iter().any(|item| item.eq_ignore_ascii_case(name)) {
      continue;
    }
    let commented = format!("; [{}]", name);
    if text_out.lines().any(|candidate| {
      candidate.trim_start().strip_prefix(';').map(str::trim).map_or(false, |value| {
        value.eq_ignore_ascii_case(commented.trim_start_matches(';').trim())
      })
    }) {
      restores.push(name.to_string());
      declared.insert(name.to_string());
    }
  }
  for name in restores {
    text_out.push_str(&format!("\n; Restored capture resource referenced by active commands\n[{}]\n", name));
  }

  let mut vertex_hashes: HashSet<String> = HashSet::new();
  let mut document = ini::parse(&text_out);
  for section in document.sections.iter_mut() {
    let lower = section.name.to_ascii_lowercase();
    if !(lower.starts_with("textureoverride") && lower.contains("vertexlimitraise")) {
      continue;
    }
    let commands = active_commands(ini::body(&section.raw));
    if commands.is_empty() {
      section.raw = comment_lines(&section.raw, "; Empty legacy vertex-limit override: ");
      continue;
    }
    if commands.len() != 1 {
      continue;
    }
    let Some((_, hash, _)) = parse_hash_line(&commands[0]) else { continue };
    if vertex_hashes.insert(hash) {
      continue;
    }
    section.raw = comment_lines(&section.raw, "; Duplicate empty vertex-limit override: ");
  }
  document.render()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn assets() -> (Rules, Migrations) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/gimi/Modules/HashRules");
    (
      Rules::load(&root.join("rules47.json")).unwrap(),
      Migrations::load(&root.join("character-migrations.json")).unwrap(),
    )
  }

  const MOD: &str = "[TextureOverrideAlbedoPosition]\r\nhash = 8747eaa2\r\nvb0 = ResourceAlbedoPosition\r\n\r\n[TextureOverrideAlbedoIB]\r\nhash = 35a9a3e9\r\nmatch_first_index = 0\r\nib = ResourceAlbedoIB\r\n";

  #[test]
  fn adapts_old_hashes_to_47() {
    let (rules, migrations) = assets();
    let converted = convert(MOD, "4.7", "", &rules, &migrations);
    assert!(!converted.blocked, "{}", converted.reason);
    assert_eq!(converted.character, "Albedo");
    assert_eq!(converted.changes, 2);
    assert!(converted.text.lines().any(|line| line.trim().eq_ignore_ascii_case("hash = 0ea2c585")));
    assert!(converted.text.lines().any(|line| line.trim().eq_ignore_ascii_case("hash = 0d7dc936")));
  }

  #[test]
  fn keeps_34_candidate_hashes() {
    let (rules, migrations) = assets();
    let converted = convert(MOD, "3.4", "", &rules, &migrations);
    assert_eq!(converted.changes, 0);
    assert!(converted.status.contains("3.4"));
  }

  #[test]
  fn blocks_unrecognised_model_hashes() {
    let (rules, migrations) = assets();
    let text = "[TextureOverrideSomethingPosition]\nhash = 12345678\nvb0 = ResourceX\n";
    let converted = convert(text, "4.7", "", &rules, &migrations);
    assert!(converted.blocked);
  }

  #[test]
  fn comments_migration_metadata_and_duplicate_vertex_limits() {
    let text = "[4.0_Updated]\nupdated = yes\n\n[TextureOverrideA]\nhash = 11111111\n\n[TextureOverrideBVertexLimitRaise]\nhash = aaaaaaaa\n\n[TextureOverrideCVertexLimitRaise]\nhash = aaaaaaaa\n";
    let result = effective(text);
    assert!(result.contains("; Migration metadata: updated = yes"));
    assert!(result.matches("; Duplicate empty vertex-limit override: hash = aaaaaaaa").count() == 1);
    let parsed = ini::parse(&result);
    assert!(parsed.section("4.0_Updated").is_none());
    assert!(parsed.section("TextureOverrideCVertexLimitRaise").is_none());
    assert!(parsed.section("TextureOverrideBVertexLimitRaise").is_some());
  }

  #[test]
  fn removes_repeated_marker_headers_and_empty_legacy_overrides() {
    let input = " [Mod_4.4]\n from44 = YES\n [Mod_4.4]\n; Migration metadata: from44 = yes\n[4.0_Updated]\n; updated = yes\n[TextureOverride41FixVertexLimitRaise]\n; hash = aaaaaaaa\n[TextureOverrideActualBody]\nhash = 12345678\nvb0 = ResourceBody\n";
    let output = effective(input);
    let document = ini::parse(&output);
    assert!(document.section("Mod_4.4").is_none());
    assert!(document.section("4.0_Updated").is_none());
    assert!(document.section("TextureOverride41FixVertexLimitRaise").is_none());
    assert!(document.section("TextureOverrideActualBody").is_some());
    assert_eq!(effective(&output), output);
  }

  #[test]
  fn restores_only_existing_commented_capture_resource_declarations() {
    let input = "[TextureOverrideBody]\nhash = 12345678\nResourceRefBody = reference ps-t2\nResourceRefHead = reference ps-t1\nResourceRefUnknown = reference ps-t3\n;[ResourceRefBody]\n;   [ResourceRefHead]\n";
    let output = effective(input);
    let document = ini::parse(&output);
    assert!(document.section("ResourceRefBody").is_some());
    assert!(document.section("ResourceRefHead").is_some());
    assert!(document.section("ResourceRefUnknown").is_none());
    assert_eq!(effective(&output), output);
  }

  #[test]
  fn prepares_views_and_report() {
    let (rules, migrations) = assets();
    let root = std::env::temp_dir().join(format!("ukiyo-gimi-prepare-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mods = root.join("Mods");
    let mod_dir = mods.join("Albedo Test");
    std::fs::create_dir_all(&mod_dir).unwrap();
    std::fs::write(mod_dir.join("mod.ini"), MOD).unwrap();
    let files = vec![mod_dir.join("mod.ini")];
    let prepared = prepare(&mods, &root, "4.7", &files, &rules, &migrations).unwrap();
    assert_eq!(prepared.files.len(), 1);
    assert!(prepared.converted.values().next().unwrap().variant.is_file());
    assert!(root.join("User/hash-variants-report.json").is_file());
    let _ = std::fs::remove_dir_all(&root);
  }
}

use super::storage::write_changed;

pub fn prepare(
  mods_root: &Path,
  app_root: &Path,
  version: &str,
  files: &[PathBuf],
  rules: &Rules,
  migrations: &Migrations,
) -> Result<Prepared, String> {
  let storage = mods_root.join("DISABLED_GIMI_Config");
  std::fs::create_dir_all(&storage).map_err(|error| error.to_string())?;
  let mut converted: HashMap<PathBuf, Converted> = HashMap::new();
  let mut groups: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();

  for file in files {
    let bytes = std::fs::read(file).map_err(|error| format!("无法读取 {}：{}", file.display(), error))?;
    let sha = digest(&bytes);
    let canonical = canonical_relative(mods_root, file);
    let id = id_for(mods_root, &canonical);
    let group = group_of(&canonical);
    let directory = storage.join("ModVersions").join(&id);
    let original = directory.join("Original").join(format!("{}.ini", sha));
    if !original.is_file() {
      std::fs::create_dir_all(original.parent().unwrap()).map_err(|error| error.to_string())?;
      write_changed(&original, &bytes)?;
    }
    let text = super::storage::decode_ini(&bytes)?;
    let mut selected: Option<Conversion> = None;
    for target in ["3.4", "4.7"] {
      let mut conversion = convert(&text, target, "", rules, migrations);
      let fixed = super::legacy_mod_fixes::apply(&conversion.text, file, target);
      if fixed != conversion.text { conversion.text = fixed; conversion.changes += 1; }
      let conversion = migrations.resources(&conversion, file, &text, target, &storage);
      let variant = directory.join(target).join(format!("{}-{}.ini", sha, rules.database.revision));
      let variant_text = if conversion.blocked {
        format!("; NEEDS MANUAL ADAPTATION - NOT SELECTED FOR LOADING\n{}", conversion.text)
      } else {
        conversion.text.clone()
      };
      if target != version {
        write_changed(&variant, &variant_text)?;
      } else {
        selected = Some(conversion);
        write_changed(&variant, &variant_text)?;
      }
    }
    let conversion = selected.unwrap_or_default();
    let variant = directory.join(version).join(format!("{}-{}.ini", sha, rules.database.revision));
    converted.insert(
      file.clone(),
      Converted { source: file.clone(), group: group.clone(), original, variant, conversion },
    );
    groups.entry(group).or_default().push(file.clone());
  }

  let mut report = Report {
    target: version.to_string(),
    revision: rules.database.revision.clone(),
    ..Default::default()
  };
  for (group, members) in &groups {
    let blocked = members.iter().find_map(|file| {
      converted.get(file).filter(|item| item.conversion.blocked).map(|item| item.conversion.clone())
    });
    if let Some(blocked) = blocked {
      for file in members {
        if let Some(item) = converted.get_mut(file) {
          item.conversion.blocked = true;
          item.conversion.status = blocked.status.clone();
          item.conversion.reason = blocked.reason.clone();
        }
      }
      report.groups.insert(group.clone(), format!("{}·未加载", blocked.status));
    } else {
      let first = converted.get(&members[0]);
      if let Some(item) = first {
        let status = if item.conversion.changes > 0 {
          format!("{} 适配·待实测", version)
        } else {
          item.conversion.status.clone()
        };
        report.groups.insert(group.clone(), status);
      }
    }
  }
  report.changed_files = converted.values().filter(|item| item.conversion.changes > 0 && !item.conversion.blocked).count();
  report.blocked_files = converted.values().filter(|item| item.conversion.blocked).count();

  let report_path = app_root.join("User").join("hash-variants-report.json");
  if let Some(parent) = report_path.parent() {
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
  }
  let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
  write_changed(&report_path, json)?;

  let mut loadable: Vec<PathBuf> = converted
    .iter()
    .filter(|(_, item)| !item.conversion.blocked)
    .map(|(file, _)| file.clone())
    .collect();
  loadable.sort();

  Ok(Prepared { version: version.to_string(), files: loadable, converted, report })
}
