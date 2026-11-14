use crate::gimi::ini;
use crate::gimi::versioned::{digest, Conversion};
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Entrance {
  #[serde(default)] pub role: String,
  #[serde(default)] pub source: String,
  #[serde(default)] pub target: String,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Character {
  #[serde(default)] pub character: String,
  #[serde(default)] pub entries: Vec<Entrance>,
  #[serde(default)] pub indexes: Vec<u32>,
  #[serde(default)] pub blend_remap: Vec<u32>,
}

#[derive(Deserialize)]
pub struct Migrations {
  pub revision: String,
  #[serde(default)] pub characters: Vec<Character>,
}

impl Migrations {
  pub fn load(path: &Path) -> Result<Self, String> {
    let raw = std::fs::read_to_string(path).map_err(|error| format!("无法读取骨骼迁移规则：{}", error))?;
    serde_json::from_str(&raw).map_err(|error| format!("骨骼迁移规则格式错误：{}", error))
  }

  pub fn can_map(&self, rule: &crate::gimi::rules::Rule) -> bool {
    self.characters.iter().any(|character| {
      character.character == rule.character
        && character.entries.iter().any(|entry| {
          entry.role == rule.role && entry.source == rule.source && entry.target == rule.target
        })
        && character.indexes == rule.indexes
    })
  }

  fn hash_present(text: &str, hash: &str) -> bool {
    text.lines().any(|line| {
      let trimmed = line.trim_start();
      if trimmed.starts_with(';') {
        return false;
      }
      let Some(equals) = trimmed.find('=') else { return false };
      trimmed[..equals].trim().eq_ignore_ascii_case("hash")
        && trimmed[equals + 1..].split(';').next().unwrap_or("").trim().eq_ignore_ascii_case(hash)
    })
  }

  pub fn resources(
    &self,
    converted: &Conversion,
    file: &Path,
    source_text: &str,
    version: &str,
    storage: &Path,
  ) -> Conversion {
    let mut result = converted.clone();
    if result.blocked {
      return result;
    }
    let characters: Vec<&str> = result.character.split(',').filter(|value| !value.is_empty()).collect();
    let selected: Vec<&Character> = self
      .characters
      .iter()
      .filter(|character| !character.blend_remap.is_empty())
      .filter(|character| characters.iter().any(|value| value.eq_ignore_ascii_case(&character.character)))
      .filter(|character| {
        character.entries.iter().any(|entry| {
          entry.role == "position_vb"
            && Self::hash_present(source_text, if version == "4.7" { &entry.source } else { &entry.target })
        })
      })
      .collect();
    if selected.is_empty() {
      return result;
    }
    if selected.len() != 1 {
      result.blocked = true;
      result.status = "需要专门适配".to_string();
      result.reason = "同一配置包含多个骨骼映射，未自动重排".to_string();
      return result;
    }
    let character = selected[0];
    let names: Vec<String> = source_text
      .lines()
      .filter_map(|line| {
        let trimmed = line.trim_start();
        if trimmed.starts_with(';') {
          return None;
        }
        let lower = trimmed.to_ascii_lowercase();
        if !lower.starts_with("vb1") {
          return None;
        }
        let equals = trimmed.find('=')?;
        let value = trimmed[equals + 1..].split(';').next().unwrap_or("").trim();
        if value.to_ascii_lowercase().starts_with("resource") {
          Some(value.to_string())
        } else {
          None
        }
      })
      .collect();
    let mut changed = 0usize;
    let mut error: Option<String> = None;
    let mut converted_doc = ini::parse(&result.text);
    for section in converted_doc.sections.iter_mut() {
      if !section.name.to_ascii_lowercase().starts_with("resource") || !names.iter().any(|name| name.eq_ignore_ascii_case(&section.name)) {
        continue;
      }
      let stride_ok = ini::get_option(&section.raw, &section.name, "stride").and_then(|value| value.parse::<u32>().ok()) == Some(32);
      if !stride_ok {
        continue;
      }
      let (header, body_text) = ini::split_header(&section.raw);
      let mut lines = String::new();
      for line in body_text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with(';') || !trimmed.to_ascii_lowercase().starts_with("filename") {
          lines.push_str(line);
          continue;
        }
        let Some(equals) = trimmed.find('=') else {
          lines.push_str(line);
          continue;
        };
        let value = trimmed[equals + 1..].split(';').next().unwrap_or("").trim().trim_matches('"');
        let path = if Path::new(value).is_absolute() {
          std::path::PathBuf::from(value)
        } else {
          file.parent().unwrap_or(Path::new(".")).join(value)
        };
        let bytes = match std::fs::read(&path) {
          Ok(bytes) => bytes,
          Err(reason) => {
            error = Some(format!("无法读取骨骼缓冲区 {}：{}", path.display(), reason));
            break;
          }
        };
        let remapped = match Self::remap(&bytes, &character.blend_remap, version == "3.4") {
          Ok(value) => value,
          Err(reason) => {
            error = Some(reason);
            break;
          }
        };
        let folder = storage.join("Buffers").join(version);
        if let Err(reason) = std::fs::create_dir_all(&folder) {
          error = Some(reason.to_string());
          break;
        }
        let alias = folder.join(format!("{}.buf", digest(&remapped)));
        if std::fs::read(&alias).map(|existing| existing != remapped).unwrap_or(true) {
          if let Err(reason) = super::storage::write_changed(&alias, &remapped) {
            error = Some(reason);
            break;
          }
        }
        let indent = &line[..line.len() - trimmed.len()];
        lines.push_str(indent);
        lines.push_str(trimmed[..equals].trim());
        lines.push_str(" = ");
        lines.push_str(&alias.to_string_lossy());
        lines.push_str(if line.ends_with("\r\n") { "\r\n" } else if line.ends_with('\n') { "\n" } else { "" });
        changed += 1;
      }
      if error.is_some() {
        break;
      }
      section.raw = format!("{}{}", header, lines);
    }
    if let Some(reason) = error {
      result.blocked = true;
      result.status = "需要专门适配".to_string();
      result.reason = reason;
      result.text = converted.text.clone();
      return result;
    }
    if changed == 0 {
      result.blocked = true;
      result.status = "需要专门适配".to_string();
      result.reason = "未找到 stride=32 的骨骼缓冲区，未自动适配。".to_string();
      result.text = converted.text.clone();
      return result;
    }
    result.text = converted_doc.render();
    result.status = format!("{} 骨骼适配·待实测", version);
    result.reason = format!(
      "使用官方 4.4 骨骼编号映射的{}副本；原缓冲区保留",
      if version == "3.4" { "反向" } else { "正向" }
    );
    result
  }

  #[cfg(test)]
  pub fn sample() -> Self {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/gimi/Modules/HashRules/character-migrations.json");
    Self::load(&root).unwrap()
  }

  /// Ports CharacterMigrations.Remap. Returns None when the buffer is not a valid blend layout.
  pub fn remap(original: &[u8], permutation: &[u32], inverse: bool) -> Result<Vec<u8>, String> {
    if original.len() % 32 != 0 || permutation.is_empty() {
      return Err("骨骼缓冲区布局或映射表无效。".to_string());
    }
    let mut sorted = permutation.to_vec();
    sorted.sort_unstable();
    if sorted != (0..permutation.len() as u32).collect::<Vec<u32>>() {
      return Err("骨骼缓冲区布局或映射表无效。".to_string());
    }
    let mut map = permutation.to_vec();
    if inverse {
      let mut reversed = vec![0u32; permutation.len()];
      for (index, value) in permutation.iter().enumerate() {
        let slot = *value as usize;
        if slot >= reversed.len() {
          return Err("骨骼映射表越界。".to_string());
        }
        reversed[slot] = index as u32;
      }
      map = reversed;
    }
    let mut output = original.to_vec();
    for offset in (0..original.len()).step_by(32) {
      for slot in 0..4 {
        let weight = f32::from_le_bytes(original[offset + slot * 4..offset + slot * 4 + 4].try_into().unwrap());
        if weight.is_nan() || weight.is_infinite() {
          return Err("骨骼权重无效。".to_string());
        }
        if weight == 0.0 {
          continue;
        }
        let index_offset = offset + 16 + slot * 4;
        let index = u32::from_le_bytes(original[index_offset..index_offset + 4].try_into().unwrap());
        let mapped = *map.get(index as usize).ok_or_else(|| "骨骼编号超出官方映射范围，需专门适配。".to_string())?;
        output[index_offset..index_offset + 4].copy_from_slice(&mapped.to_le_bytes());
      }
    }
    Ok(output)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn blend_remap_round_trips() {
    let data = Migrations::sample();
    let character = data
      .characters
      .iter()
      .find(|character| !character.blend_remap.is_empty())
      .expect("migration data");
    assert!(character.blend_remap.len() >= 4);
    let mut buffer = vec![0u8; 32];
    buffer[0..4].copy_from_slice(&1.0f32.to_le_bytes());
    for slot in 0..4 {
      let index = slot as u32;
      buffer[16 + slot * 4..20 + slot * 4].copy_from_slice(&index.to_le_bytes());
    }
    let mapped = Migrations::remap(&buffer, &character.blend_remap, false).unwrap();
    let back = Migrations::remap(&mapped, &character.blend_remap, true).unwrap();
    assert_eq!(back, buffer);
  }
}
