use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Rule {
  #[serde(default)] pub character: String,
  #[serde(default)] pub component: String,
  #[serde(default)] pub role: String,
  #[serde(default)] pub source: String,
  #[serde(default)] pub target: String,
  #[serde(default)] pub reference: String,
  #[serde(default)] pub layout_consistent: bool,
  #[serde(default)] pub indexes: Vec<u32>,
}

#[derive(Deserialize)]
pub struct Database {
  pub revision: String,
  #[serde(default)] pub target: String,
  #[serde(default)] pub characters34: Vec<String>,
  #[serde(default)] pub rules: Vec<Rule>,
}

pub struct Rules {
  pub database: Database,
  by_hash: HashMap<String, Vec<usize>>,
}

impl Rules {
  pub fn load(path: &std::path::Path) -> Result<Self, String> {
    let raw = std::fs::read_to_string(path).map_err(|error| format!("无法读取哈希规则：{}", error))?;
    let database: Database = serde_json::from_str(&raw).map_err(|error| format!("哈希规则格式错误：{}", error))?;
    let mut by_hash: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, rule) in database.rules.iter().enumerate() {
      let source = rule.source.to_ascii_lowercase();
      let target = rule.target.to_ascii_lowercase();
      by_hash.entry(source).or_default().push(index);
      if target != rule.source.to_ascii_lowercase() {
        by_hash.entry(target).or_default().push(index);
      }
    }
    Ok(Rules { database, by_hash })
  }

  pub fn candidates(&self, hash: &str) -> &[usize] {
    self.by_hash.get(&hash.to_ascii_lowercase()).map(|value| value.as_slice()).unwrap_or(&[])
  }

  pub fn rule(&self, index: usize) -> &Rule {
    &self.database.rules[index]
  }

  pub fn has_character34(&self, character: &str) -> bool {
    self.database.characters34.iter().any(|value| value.eq_ignore_ascii_case(character))
  }

  pub fn is_anchor(role: &str) -> bool {
    matches!(role, "position_vb" | "blend_vb" | "texcoord_vb" | "draw_vb" | "ib")
  }
}
