//! Verified mod configuration repairs shared by 3.4 and 4.7 runtime views.
use super::ini;
use std::path::Path;

fn same_file_content(left: &Path, right: &Path) -> bool {
  match (std::fs::read(left), std::fs::read(right)) {
    (Ok(left), Ok(right)) => left == right,
    _ => false,
  }
}

pub fn apply(text: &str, source: &Path, target: &str) -> String {
  if target != "3.4" && target != "4.7" { return text.to_string(); }
  let Some(directory) = source.parent() else { return text.to_string(); };
  let folder = directory.file_name().and_then(|name| name.to_str()).unwrap_or_default();
  let file = source.file_name().and_then(|name| name.to_str()).unwrap_or_default();
  let mut result = text.to_string();
  if folder == "妮露_友谊_切换" && file.eq_ignore_ascii_case("merged.ini")
    && ini::parse(text).section("ResourceNilouBodyMetalMap.9").is_none()
    && text.lines().any(|line| line.trim().eq_ignore_ascii_case("ps-t3 = ResourceNilouBodyMetalMap.9")) {
    if let Some(filename) = ini::get_option(text, "ResourceNilouBodyMetalMap.8", "filename") {
      let donor = directory.join(&filename);
      let expected = directory.join("Nilou Friendship 10/NilouBodyMetalMap.dds");
      if same_file_content(&donor, &expected) {
        result = ini::set_option(&result, "ResourceNilouBodyMetalMap.9", "filename", &filename);
      }
    }
  }
  if folder == "安柏_半裸-CN" && file.eq_ignore_ascii_case("Amber.ini")
    && ini::get_option(text, "TextureOverrideAmberFaceHeadDiffuse", "hash").as_deref() == Some("1d064079")
    && ini::get_option(text, "TextureOverrideAmberFaceHeadDiffuse", "ps-t0").as_deref() == Some("ResourceAmberFaceHeadDiffuse")
    && ini::get_option(text, "TextureOverrideAmberFaceHeadDiffuse", "match_priority").is_none() {
    if let (Some(filename), Some(mods)) = (ini::get_option(text, "ResourceAmberFaceHeadDiffuse", "filename"), directory.parent()) {
      let original = mods.join("安柏_半裸/Amber.ini");
      if let Ok(original_text) = super::storage::read_ini(&original) {
        if ini::get_option(&original_text, "TextureOverrideAmberFaceHeadDiffuse", "hash").as_deref() == Some("1d064079")
          && ini::get_option(&original_text, "TextureOverrideAmberFaceHeadDiffuse", "ps-t0").as_deref() == Some("ResourceAmberFaceHeadDiffuse")
          && ini::get_option(&original_text, "TextureOverrideAmberFaceHeadDiffuse", "match_priority").is_none() {
          if let Some(other) = ini::get_option(&original_text, "ResourceAmberFaceHeadDiffuse", "filename") {
            if same_file_content(&directory.join(filename), &mods.join("安柏_半裸").join(other)) {
              result = ini::set_option(&result, "TextureOverrideAmberFaceHeadDiffuse", "match_priority", "1");
            }
          }
        }
      }
    }
  }
  result
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn fixes_both_versions_with_verified_existing_textures() {
    let root = std::env::temp_dir().join(format!("ukiyo-34-mod-fixes-{}", std::process::id()));
    let nilou = root.join("妮露_友谊_切换");
    std::fs::create_dir_all(nilou.join("Nilou Friendship 10")).unwrap();
    std::fs::write(nilou.join("metal.dds"), b"metal").unwrap();
    std::fs::write(nilou.join("Nilou Friendship 10/NilouBodyMetalMap.dds"), b"metal").unwrap();
    let text = "[CommandListBody]\nps-t3 = ResourceNilouBodyMetalMap.9\n[ResourceNilouBodyMetalMap.8]\nfilename = metal.dds\n";
    let source = nilou.join("merged.ini");
    std::fs::write(&source, text).unwrap();
    let repaired = apply(text, &source, "3.4");
    assert_eq!(ini::get_option(&repaired, "ResourceNilouBodyMetalMap.9", "filename").as_deref(), Some("metal.dds"));
    assert_eq!(apply(text, &source, "4.7"), repaired);
    for version in ["3.4", "4.7"] { assert_eq!(apply(&repaired, &source, version), repaired); }
    assert_eq!(apply(text, &source, "5.0"), text);
    std::fs::write(nilou.join("metal.dds"), b"different").unwrap();
    for version in ["3.4", "4.7"] { assert_eq!(apply(text, &source, version), text); }
    assert_eq!(std::fs::read_to_string(source).unwrap(), text);

    let amber = "[TextureOverrideAmberFaceHeadDiffuse]\nhash = 1d064079\nps-t0 = ResourceAmberFaceHeadDiffuse\n[ResourceAmberFaceHeadDiffuse]\nfilename = face.dds\n";
    for folder in ["安柏_半裸", "安柏_半裸-CN"] {
      std::fs::create_dir_all(root.join(folder)).unwrap();
      std::fs::write(root.join(folder).join("Amber.ini"), amber).unwrap();
      std::fs::write(root.join(folder).join("face.dds"), b"face").unwrap();
    }
    let source = root.join("安柏_半裸-CN/Amber.ini");
    let repaired = apply(amber, &source, "3.4");
    assert_eq!(ini::get_option(&repaired, "TextureOverrideAmberFaceHeadDiffuse", "match_priority").as_deref(), Some("1"));
    assert_eq!(apply(amber, &source, "4.7"), repaired);
    for version in ["3.4", "4.7"] { assert_eq!(apply(&repaired, &source, version), repaired); }
    assert_eq!(apply(amber, &source, "5.0"), amber);
    std::fs::write(root.join("安柏_半裸-CN/face.dds"), b"different").unwrap();
    for version in ["3.4", "4.7"] { assert_eq!(apply(amber, &source, version), amber); }
    assert_eq!(std::fs::read_to_string(source).unwrap(), amber);
    std::fs::remove_dir_all(root).unwrap();
  }
}
