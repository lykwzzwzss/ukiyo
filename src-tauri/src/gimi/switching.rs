//! Runtime selection of complete mod folders; source INIs and outfit variables
//! remain untouched. Only confidently identified, compatible models participate.
use super::{ini, rules::Rules, versioned::{digest, Prepared}};
use serde::Serialize;
use std::{collections::{BTreeMap, BTreeSet, HashMap}, path::PathBuf};

#[derive(Clone, Serialize)]
pub struct Choice {
  pub group: String,
  pub token: u32,
}

#[derive(Serialize)]
pub struct Family {
  pub character: String,
  pub id: String,
  pub choices: Vec<Choice>,
  #[serde(skip)]
  anchors: BTreeSet<String>,
}

#[derive(Default, Serialize)]
pub struct Plan {
  pub families: Vec<Family>,
  pub skipped: BTreeMap<String, String>,
  #[serde(skip)]
  files: HashMap<PathBuf, (usize, u32)>,
}

fn assignment(line: &str) -> Option<(&str, &str)> {
  let line = line.split(';').next()?.trim();
  let (key, value) = line.split_once('=')?;
  Some((key.trim(), value.trim()))
}

fn unsupported(text: &str) -> bool {
  ini::parse(text).sections.iter().any(|section| {
    let name = section.name.to_ascii_lowercase();
    // Regex shader replacement and presets may act independently of the normal
    // command lists. Do not pretend that command gating disables those effects.
    if name.starts_with("shaderregex") || name.starts_with("preset") { return true; }
    if name.starts_with("textureoverride") {
      return ini::body(&section.raw).lines().filter_map(assignment).any(|(key, _)| {
        matches!(key.to_ascii_lowercase().as_str(), "format" | "width" | "height" | "width_multiply" | "height_multiply" | "stereomode" | "iteration" | "expand_region_copy" | "deny_cpu_read")
      });
    }
    false
  })
}

pub fn build(prepared: &Prepared, rules: &Rules) -> Plan {
  let mut plan = Plan::default();
  let mut folders: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
  for file in &prepared.files {
    if let Some(item) = prepared.converted.get(file) {
      folders.entry(item.group.clone()).or_default().push(file.clone());
    }
  }
  let mut characters: BTreeMap<String, Vec<String>> = BTreeMap::new();
  for (folder, files) in &folders {
    let names: BTreeSet<_> = files.iter().filter_map(|file| prepared.converted.get(file))
      .flat_map(|item| item.conversion.character.split(','))
      .filter(|name| !name.is_empty()).map(str::to_string).collect();
    if names.len() != 1 { continue; }
    characters.entry(names.into_iter().next().unwrap()).or_default().push(folder.clone());
  }
  for (character, groups) in characters {
    if groups.len() < 2 { continue; }
    if groups.iter().any(|group| folders[group].iter().any(|file| unsupported(&prepared.converted[file].conversion.text))) {
      plan.skipped.insert(character, "包含独立着色器替换或静态资源修改，未自动合并轮换".into());
      continue;
    }
    let anchors: BTreeSet<_> = rules.database.rules.iter()
      .filter(|rule| rule.character == character && Rules::is_anchor(&rule.role))
      .map(|rule| if prepared.version == "3.4" { rule.source.clone() } else { rule.target.clone() })
      .map(|hash| hash.to_ascii_lowercase()).collect();
    if anchors.is_empty() { continue; }
    let family_index = plan.families.len();
    let mut tokens = BTreeSet::new();
    let mut choices = Vec::new();
    for group in groups {
      // Small stable integer IDs are exactly representable by Migoto floats.
      // Keeping IDs stable avoids changing the selected folder when another is
      // added or removed. Resolve the unlikely collision within this character.
      let mut token = u32::from_str_radix(&digest(group.to_lowercase().as_bytes())[..5], 16).unwrap();
      while !tokens.insert(token) { token = (token + 1) & 0xfffff; }
      for file in &folders[&group] { plan.files.insert(file.clone(), (family_index, token)); }
      choices.push(Choice { group, token });
    }
    plan.families.push(Family { id: digest(character.as_bytes())[..12].into(), character, choices, anchors });
  }
  plan
}

impl Plan {
  pub fn manager(&self) -> String {
    let mut text = String::from("; Managed whole-mod switching. Do not edit.\nnamespace = ukiyo\\ModSwitch\n\n[Constants]\n");
    for family in &self.families {
      text.push_str(&format!("global persist $c_{} = {}\nglobal $seen_{} = 0\n", family.id, family.choices[0].token, family.id));
    }
    text.push_str("\n[Present]\n");
    for family in &self.families {
      // Removed selections must fall back to a remaining mod, never a blank model.
      let invalid = family.choices.iter().map(|choice| format!("$c_{} != {}", family.id, choice.token)).collect::<Vec<_>>().join(" && ");
      text.push_str(&format!("if {}\n  $c_{} = {}\nendif\npost $seen_{} = 0\n", invalid, family.id, family.choices[0].token, family.id));
    }
    for family in &self.families {
      let values = family.choices.iter().map(|choice| choice.token.to_string()).collect::<Vec<_>>().join(", ");
      text.push_str(&format!("\n[KeyCycle{}]\nkey = no_modifiers VK_OEM_2\ncondition = $costume_mods && $seen_{}\ntype = cycle\n$c_{} = {}\n", family.id, family.id, family.id, values));
    }
    text
  }

  pub fn apply(&self, file: &std::path::Path, text: &str) -> String {
    let Some(&(family_index, token)) = self.files.get(file) else { return text.into() };
    let family = &self.families[family_index];
    let selected = format!("$costume_mods && $\\ukiyo\\ModSwitch\\c_{} == {}", family.id, token);
    let mut document = ini::parse(text);
    for section in &mut document.sections {
      let name = section.name.to_ascii_lowercase();
      if name.starts_with("key") {
        let mut condition = selected.clone();
        if let Some(original) = ini::get_option(&section.raw, &section.name, "condition") {
          condition = format!("({}) && ({})", original, condition);
        }
        // Slash is reserved for whole-mod switching in a managed family.
        if ini::body(&section.raw).lines().filter_map(assignment).any(|(key, value)| {
          key.eq_ignore_ascii_case("key") && value.split_whitespace().any(|word| word.eq_ignore_ascii_case("VK_OEM_2") || word == "/")
        }) { condition.push_str(" && 0"); }
        section.raw = ini::set_option(&section.raw, &section.name, "condition", &condition);
        continue;
      }
      let texture = name.starts_with("textureoverride");
      let shader = name.starts_with("shaderoverride");
      if !texture && !shader && !name.starts_with("commandlist") && name != "present" { continue; }
      let (header, body) = ini::split_header(&section.raw);
      let mut options = String::new();
      let mut commands = String::new();
      let mut anchor = false;
      let mut has_priority = false;
      let mut has_duplicates = false;
      for line in body.lines() {
        let static_option = assignment(line).map_or(false, |(key, value)| {
          let key = key.to_ascii_lowercase();
          if texture && key == "hash" { anchor |= family.anchors.contains(&value.to_ascii_lowercase()); }
          has_priority |= key == "match_priority";
          has_duplicates |= key == "allow_duplicate_hash";
          (texture && (key == "hash" || key == "filter_index" || key.starts_with("match_")))
            || (shader && matches!(key.as_str(), "hash" | "allow_duplicate_hash" | "depth_filter" | "partner" | "filter_index"))
        });
        if static_option { options.push_str(line); options.push('\n'); }
        else { commands.push_str(line); commands.push('\n'); }
      }
      // All TextureOverrides sharing a hash execute; a priority explicitly marks
      // this as intentional. Gating the commands makes exactly one folder active.
      if texture && !has_priority { options.push_str("match_priority = 0\n"); }
      if shader && !has_duplicates { options.push_str("allow_duplicate_hash = true\n"); }
      if anchor { options.push_str(&format!("$\\ukiyo\\ModSwitch\\seen_{} = 1\n", family.id)); }
      section.raw = format!("{}{}if {}\n{}endif\n\n", header, options, selected, commands);
    }
    document.render()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use super::super::versioned::{Converted, Conversion, Report};

  fn fixture() -> (Prepared, Rules) {
    let rules = Rules::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/gimi/Modules/HashRules/rules47.json")).unwrap();
    let mut prepared = Prepared { version: "4.7".into(), files: Vec::new(), converted: HashMap::new(), report: Report::default() };
    for (file, group, character) in [("A/main.ini", "A", "Albedo"), ("A/extra.ini", "A", ""), ("B/main.ini", "B", "Albedo"), ("C/main.ini", "C", "Amber")] {
      let path = PathBuf::from(file);
      prepared.files.push(path.clone());
      prepared.converted.insert(path.clone(), Converted { source: path, group: group.into(), original: PathBuf::new(), variant: PathBuf::new(), conversion: Conversion { character: character.into(), ..Default::default() } });
    }
    (prepared, rules)
  }

  #[test]
  fn folders_are_choices_not_files_and_removed_ids_are_repaired() {
    let (prepared, rules) = fixture();
    let plan = build(&prepared, &rules);
    assert_eq!(plan.families.len(), 1);
    assert_eq!(plan.families[0].choices.len(), 2);
    assert_eq!(plan.files[&PathBuf::from("A/main.ini")], plan.files[&PathBuf::from("A/extra.ini")]);
    assert!(!plan.files.contains_key(&PathBuf::from("C/main.ini")));
    let manager = plan.manager();
    assert_eq!(manager.matches("[KeyCycle").count(), 1);
    assert!(manager.contains("VK_OEM_2"));
    assert!(manager.contains("post $seen_"));
    assert!(manager.contains(" != "));
    let mut removed = prepared;
    removed.files.retain(|p| !p.starts_with("A"));
    assert!(build(&removed, &rules).families.is_empty());
  }

  #[test]
  fn replacement_draws_keys_and_present_are_guarded_but_detection_is_not() {
    let (prepared, rules) = fixture();
    let plan = build(&prepared, &rules);
    let text = "namespace = Original\n[Constants]\nglobal persist $outfit = 2\n[TextureOverridePosition]\nhash = df65bb00\nvb0 = ResourcePosition\n[KeyOutfit]\nkey = VK_OEM_5\ncondition = $active\n$outfit = 0,1,2\n[Present]\npost $active = 0\n[CommandListDraw]\nhandling = skip\ndrawindexed = auto\n[ResourcePosition]\nfilename = position.buf\n";
    let result = plan.apply(std::path::Path::new("A/main.ini"), text);
    let doc = ini::parse(&result);
    let position = &doc.section("TextureOverridePosition").unwrap().raw;
    assert!(position.find("seen_").unwrap() < position.find("if $costume_mods").unwrap());
    assert!(position.find("if $costume_mods").unwrap() < position.find("vb0 =").unwrap());
    assert!(position.contains("match_priority = 0"));
    assert!(doc.section("CommandListDraw").unwrap().raw.contains("if $costume_mods"));
    assert!(doc.section("Present").unwrap().raw.contains("if $costume_mods"));
    assert!(doc.section("KeyOutfit").unwrap().raw.contains("condition = ($active) && ("));
    assert!(result.contains("$outfit = 0,1,2"));
    assert_eq!(doc.section("ResourcePosition").unwrap().raw, "[ResourcePosition]\nfilename = position.buf\n");
    assert_eq!(plan.apply(std::path::Path::new("C/main.ini"), text), text);
  }

  #[test]
  fn unsupported_independent_shader_replacements_are_not_silently_switched() {
    let (mut prepared, rules) = fixture();
    prepared.converted.get_mut(&PathBuf::from("B/main.ini")).unwrap().conversion.text = "[ShaderRegexTest]\nshader_model = ps_5_0\n".into();
    let plan = build(&prepared, &rules);
    assert!(plan.families.is_empty());
    assert!(plan.skipped.contains_key("Albedo"));
  }
}
