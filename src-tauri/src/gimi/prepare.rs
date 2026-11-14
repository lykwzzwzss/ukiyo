use crate::gimi::{autolibs, ini, migrations::Migrations, rules::Rules, versioned};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PrepareSummary {
  pub version: String,
  pub core: String,
  pub mods: String,
  pub managed_mod_inis: usize,
  pub ignored_duplicates: usize,
  pub changed_files: usize,
  pub blocked_files: usize,
  pub texfx: bool,
  pub offset: bool,
  pub land: String,
  pub summary: String,
  pub groups: std::collections::BTreeMap<String, String>,
}

fn windows_lines(text: &str) -> String {
  text.replace("\r\n", "\n").replace('\r', "\n").replace('\n', "\r\n")
}

use super::storage::write_changed;

fn persistent_file(root: &Path, core: &str, version: &str) -> Result<String, String> {
  let tag = if version.is_empty() { "Unknown" } else { version };
  let directory = root.join("User").join("ModState").join(core);
  std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
  let original = root.join("Profiles").join(core).join("d3dx_user.ini");
  for target in ["3.4", "4.7"] {
    let path = directory.join(format!("{}.ini", target));
    if !path.exists() {
      let seed = std::fs::read_to_string(&original).unwrap_or_else(|_| "[Constants]\n".to_string());
      std::fs::write(&path, seed).map_err(|error| error.to_string())?;
    }
  }
  Ok(format!("..\\..\\User\\ModState\\{}\\{}.ini", core, tag))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
      let target = to.join(entry.file_name());
      if entry.path().is_dir() {
        copy_dir(&entry.path(), &target);
      } else {
        std::fs::copy(entry.path(), target).unwrap();
      }
    }
  }

  #[test]
  fn prepares_a_real_workspace() {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/gimi");
    let root = std::env::temp_dir().join(format!("ukiyo-gimi-workspace-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    copy_dir(&assets.join("Modules/HashRules"), &root.join("Modules/HashRules"));
    std::fs::create_dir_all(root.join("Modules/Auto")).unwrap();
    std::fs::create_dir_all(root.join("Modules/Auto/47/PixelShaders")).unwrap();
    std::fs::copy(assets.join("Modules/Auto/land-shader-hashes.json"), root.join("Modules/Auto/land-shader-hashes.json")).unwrap();
    for core in ["XXMI"] {
      let profile = root.join("Profiles").join(core);
      std::fs::create_dir_all(&profile).unwrap();
      std::fs::create_dir_all(profile.join("ShaderFixes")).unwrap();
      std::fs::write(profile.join("template.ini"), "[Include]\n[Loader]\n[Include ManagedMods]\n[Rendering]\n[Logging]\n[Hunting]\n").unwrap();
    }
    let game_dir = root.join("Game");
    std::fs::create_dir_all(game_dir.join("GenshinImpact_Data")).unwrap();
    std::fs::write(game_dir.join("GenshinImpact.exe"), "").unwrap();
    std::fs::write(game_dir.join("GenshinImpact_Data/globalgamemanagers"), "4.7.0_1234567").unwrap();
    let mods = root.join("Mods");
    let mod_dir = mods.join("Albedo Test");
    std::fs::create_dir_all(&mod_dir).unwrap();
    std::fs::write(
      mod_dir.join("mod.ini"),
      "[TextureOverrideAlbedoPosition]\r\nhash = 8747eaa2\r\nvb0 = ResourceAlbedoPosition\r\n\r\n[TextureOverrideAlbedoIB]\r\nhash = 35a9a3e9\r\nib = ResourceAlbedoIB\r\n",
    )
    .unwrap();

    let summary = prepare(&root, &game_dir.join("GenshinImpact.exe"), &mods).unwrap();
    assert_eq!(summary.version, "4.7");
    assert_eq!(summary.managed_mod_inis, 1);
    assert!(root.join("Profiles/XXMI/d3dx.ini").is_file());
    assert!(root.join("User/VersionViews/XXMI/4.7").read_dir().unwrap().next().is_some());
    assert!(root.join("User/hash-variants-report.json").is_file());
    let catalog = std::fs::read_to_string(root.join("Profiles/XXMI/mod-catalog.ini")).unwrap();
    assert!(catalog.contains("VersionViews"));
    let config = std::fs::read_to_string(root.join("Profiles/XXMI/d3dx.ini")).unwrap();
    assert!(config.contains("mod-catalog.ini"));
    assert!(config.contains("..\\..\\User\\RuntimeShaders"));

    // Entire unsafe groups must be excluded from the actual loading catalog,
    // including otherwise valid sibling INIs in that group.
    let blocked = mods.join("Unsafe");
    std::fs::create_dir_all(&blocked).unwrap();
    std::fs::write(blocked.join("bad.ini"), "[TextureOverrideSomethingPosition]\nhash = 12345678\nvb0 = ResourceX\n").unwrap();
    std::fs::write(blocked.join("good.ini"), std::fs::read(mod_dir.join("mod.ini")).unwrap()).unwrap();
    let source_before = std::fs::read(mod_dir.join("mod.ini")).unwrap();
    let summary = prepare(&root, &game_dir.join("GenshinImpact.exe"), &mods).unwrap();
    assert_eq!(summary.blocked_files, 2);
    let catalog = std::fs::read_to_string(root.join("Profiles/XXMI/mod-catalog.ini")).unwrap();
    assert_eq!(catalog.matches("[Include Managed").count(), 1);
    assert_eq!(std::fs::read_dir(root.join("User/VersionViews/XXMI/4.7")).unwrap().count(), 1);

    crate::gimi::mods::toggle_mod(&mods, "Albedo Test", false).unwrap();
    prepare(&root, &game_dir.join("GenshinImpact.exe"), &mods).unwrap();
    assert!(!std::fs::read_to_string(root.join("Profiles/XXMI/mod-catalog.ini")).unwrap().contains("[Include Managed"));
    crate::gimi::mods::toggle_mod(&mods, "DISABLED_Albedo Test", true).unwrap();
    prepare(&root, &game_dir.join("GenshinImpact.exe"), &mods).unwrap();
    assert_eq!(std::fs::read(mod_dir.join("mod.ini")).unwrap(), source_before);
    assert_eq!(std::fs::read_to_string(root.join("Profiles/XXMI/mod-catalog.ini")).unwrap().matches("[Include Managed").count(), 1);
    assert!(!root.join("Profiles/V7").exists());

    let second = mods.join("Albedo Second");
    std::fs::create_dir_all(&second).unwrap();
    std::fs::write(second.join("mod.ini"), &source_before).unwrap();
    for version in ["3.4", "4.7"] {
      std::fs::write(game_dir.join("GenshinImpact_Data/globalgamemanagers"), format!("{}.0_1234567", version)).unwrap();
      prepare(&root, &game_dir.join("GenshinImpact.exe"), &mods).unwrap();
      let catalog = std::fs::read_to_string(root.join("Profiles/XXMI/mod-catalog.ini")).unwrap();
      assert!(catalog.contains("[Include UkiyoModSwitch]"));
      let manager = std::fs::read_to_string(root.join("User/mod-switching.ini")).unwrap();
      assert_eq!(manager.matches("[KeyCycle").count(), 1);
      for entry in std::fs::read_dir(root.join("User/VersionViews/XXMI").join(version)).unwrap() {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        assert!(text.contains("if $costume_mods && $\\ukiyo\\ModSwitch\\c_"));
        assert!(text.contains("match_priority = 0"));
      }
    }
    crate::gimi::mods::toggle_mod(&mods, "Albedo Second", false).unwrap();
    prepare(&root, &game_dir.join("GenshinImpact.exe"), &mods).unwrap();
    assert!(!std::fs::read_to_string(root.join("Profiles/XXMI/mod-catalog.ini")).unwrap().contains("UkiyoModSwitch"));
    assert_eq!(std::fs::read(mod_dir.join("mod.ini")).unwrap(), source_before);

    // Exercise the actual startup/reload/launch preparation coordinator.
    use crate::gimi::{PREPARATION_RUNS, warm_workspace, ensure_prepared, prepare_workspace, selection};
    use std::sync::atomic::Ordering;
    let runs = || PREPARATION_RUNS.load(Ordering::SeqCst);
    let game = game_dir.join("GenshinImpact.exe");
    assert!(warm_workspace(&root, &game, &mods).unwrap());
    let warmed = runs();
    assert!(warm_workspace(&root, &game, &mods).unwrap());
    ensure_prepared(&root, &game, &mods).unwrap();
    assert_eq!(runs(), warmed, "launch should reuse background preparation");
    selection::stage(&root, &mods, "Albedo Test", false).unwrap();
    assert!(!warm_workspace(&root, &game, &mods).unwrap());
    assert!(mod_dir.is_dir(), "background preparation must not apply pending choices");
    ensure_prepared(&root, &game, &mods).unwrap();
    assert_eq!(runs(), warmed + 1, "launch applies pending choices exactly once");
    let rebuilt = runs();
    ensure_prepared(&root, &game, &mods).unwrap();
    assert_eq!(runs(), rebuilt);
    selection::stage(&root, &mods, "DISABLED_Albedo Test", true).unwrap();
    prepare_workspace(&root, &game, &mods).unwrap();
    let reloaded = runs();
    ensure_prepared(&root, &game, &mods).unwrap();
    assert_eq!(runs(), reloaded, "manual reload should populate the same cache");
    for version in ["3.4", "4.7", "3.4"] {
      std::fs::write(game_dir.join("GenshinImpact_Data/globalgamemanagers"), format!("{}.0_1234567", version)).unwrap();
      let before = runs();
      assert!(warm_workspace(&root, &game, &mods).unwrap());
      assert_eq!(runs(), before + 1, "version switch needs the matching active catalog");
      assert_eq!(ensure_prepared(&root, &game, &mods).unwrap().version, version);
      assert_eq!(runs(), before + 1);
    }
    let _ = std::fs::remove_dir_all(&root);
  }
}

pub fn prepare(root: &Path, game: &Path, mods: &Path) -> Result<PrepareSummary, String> {
  let core = "XXMI";
  if !game.is_file() {
    return Err("游戏路径无效，请先在设置里选择游戏程序。".to_string());
  }
  if !mods.is_dir() {
    return Err("模组目录不存在，请重新选择。".to_string());
  }
  fn clean(path: std::path::PathBuf) -> std::path::PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix("\\\\?\\") {
      Some(rest) => std::path::PathBuf::from(rest),
      None => path,
    }
  }
  let mods_absolute = clean(std::fs::canonicalize(mods).unwrap_or_else(|_| mods.to_path_buf()));
  let root_absolute = clean(std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf()));
  if mods_absolute.starts_with(&root_absolute) && !mods_absolute.starts_with(root_absolute.join("Mods")) {
    return Err("请选择存放角色模组的目录，避免包含启动器及其运行配置。".to_string());
  }

  let rules = Rules::load(&root.join("Modules/HashRules/rules47.json"))?;
  let migrations = Migrations::load(&root.join("Modules/HashRules/character-migrations.json"))?;
  let mut plan = autolibs::analyze(&mods_absolute, game)?;
  if plan.version.is_empty() {
    return Err("未能识别游戏版本（仅支持 3.4 / 4.7 客户端）。".to_string());
  }
  let prepared = versioned::prepare(&mods_absolute, root, &plan.version, &plan.files, &rules, &migrations)?;

  let effective_texts: Vec<(PathBuf, String)> = prepared
    .files
    .iter()
    .filter_map(|file| prepared.converted.get(file).map(|item| (file.clone(), item.conversion.text.clone())))
    .collect();
  autolibs::recalculate(&mut plan, &effective_texts);

  write_changed(&root.join("User/addons.ini"), &windows_lines(&autolibs::includes(&plan)))?;

  let summary_text = if plan.version.is_empty() {
    "尚未识别游戏版本".to_string()
  } else {
    format!(
      "原神 {}{}",
      plan.version,
      if plan.texfx { " · TexFX" } else { "" }
    )
  };
  let compatibility = format!(
    "{}\n\n反射与描边修复、模型偏移接口、血量接口：内置提供\n状态与地区检测：随游戏版本选择\n陆地透明过滤：{}\n水下透明过滤：{}\n外部重复配置已跳过：{} 份（原文件未改动）\n\n模组启停后按 F10 重载。\n",
    summary_text,
    if plan.land == "legacy46" { "4.6 传统方案（4.7 待实机确认）" } else if plan.texfx { "TexFX" } else { "等待识别游戏" },
    if plan.version == "4.7" { "已内置，待实机确认" } else { "3.4 无枫丹潜水" },
    plan.ignored.len()
  );
  write_changed(&root.join("User/compatibility.txt"), &windows_lines(&compatibility))?;
  let libraries = serde_json::json!({
    "version": plan.version,
    "texfx": plan.texfx,
    "offset": plan.offset,
    "land": plan.land,
    "managed_mod_inis": plan.files.len(),
    "ignored_duplicates": plan.ignored.len(),
    "summary": summary_text,
  });
  write_changed(&root.join("User/automatic-libraries.json"), &serde_json::to_string_pretty(&libraries).unwrap_or_default())?;

  for current_core in ["XXMI"] {
    let template_path = root.join("Profiles").join(current_core).join("template.ini");
    let base = std::fs::read_to_string(&template_path)
      .or_else(|_| std::fs::read_to_string(root.join("Profiles").join(current_core).join("d3dx.ini")))
      .map_err(|error| format!("无法读取 {} 模板：{}", current_core, error))?;
    autolibs::native_index(root, current_core, &mods_absolute, &plan, &prepared, &rules)?;
    let mut config = ini::set_option(&base, "Include", "user_config", &persistent_file(root, current_core, &plan.version)?);
    let target = game.to_string_lossy().into_owned();
    config = ini::set_option(&config, "Loader", "target", &target);
    config = config
      .lines()
      .map(|line| {
        if line.trim_start().to_ascii_lowercase().starts_with("include_recursive") {
          "; Managed catalog replaces recursive Mods loading".to_string()
        } else {
          line.to_string()
        }
      })
      .collect::<Vec<String>>()
      .join("\n");
    config = ini::set_option(&config, "Include ManagedMods", "include", "mod-catalog.ini");
    config = ini::set_option(&config, "Rendering", "override_directory", &autolibs::shaders(root, current_core, &plan)?);
    config = ini::set_option(&config, "Rendering", "dump_usage", "0");
    config = ini::set_option(&config, "Rendering", "export_shaders", "0");
    config = ini::set_option(&config, "Rendering", "export_hlsl", "0");
    config = ini::set_option(&config, "Logging", "calls", "0");
    config = ini::set_option(&config, "Hunting", "hunting", "0");
    if current_core == "XXMI" {
      config = ini::set_option(&config, "Logging", "log_level", "warning");
    }
    write_changed(&root.join("Profiles").join(current_core).join("d3dx.ini"), &windows_lines(&config))?;
  }

  // Do not overwrite the live game's persistent variables during a Mod toggle.
  if !crate::game_lifecycle::is_started() {
    for current_core in ["XXMI"] {
      crate::gimi::shared::prepare(root, current_core, &plan.version, &prepared.files, &mods_absolute)?;
    }
  }

  Ok(PrepareSummary {
    version: plan.version.clone(),
    core: core.to_string(),
    mods: mods_absolute.to_string_lossy().into_owned(),
    managed_mod_inis: plan.files.len(),
    ignored_duplicates: plan.ignored.len(),
    changed_files: prepared.report.changed_files,
    blocked_files: prepared.report.blocked_files,
    texfx: plan.texfx,
    offset: plan.offset,
    land: plan.land.clone(),
    summary: summary_text,
    groups: prepared.report.groups.clone(),
  })
}
