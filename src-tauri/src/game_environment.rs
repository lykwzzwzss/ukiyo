//! Override proxy routing only in the game child's environment.
use std::process::Command;

fn overrides(proxy: Option<&str>, run_as_invoker: bool) -> Vec<(&'static str, Option<String>)> {
  let mut values = Vec::new();
  if let Some(proxy) = proxy {
    values.extend([
      ("HTTP_PROXY", Some(proxy.to_string())),
      ("HTTPS_PROXY", Some(proxy.to_string())),
      ("ALL_PROXY", None),
      ("NO_PROXY", None),
    ]);
  }
  if run_as_invoker { values.push(("__COMPAT_LAYER", Some("RUNASINVOKER".into()))); }
  values
}

pub fn apply(command: &mut Command, proxy: Option<&str>, run_as_invoker: bool) {
  for (name, value) in overrides(proxy, run_as_invoker) {
    // Windows names are case-insensitive. Remove lowercase aliases as well for
    // tools with POSIX-style environment handling.
    command.env_remove(name.to_ascii_lowercase());
    match value { Some(value) => { command.env(name, value); }, None => { command.env_remove(name); } }
  }
}

pub fn desktop_block(mut variables: Vec<Vec<u16>>, proxy: Option<&str>, run_as_invoker: bool) -> Vec<u16> {
  for (name, value) in overrides(proxy, run_as_invoker) {
    variables.retain(|variable| {
      let variable = String::from_utf16_lossy(variable);
      !variable.split_once('=').map_or(false, |(key, _)| key.eq_ignore_ascii_case(name))
    });
    if let Some(value) = value { variables.push(format!("{}={}", name, value).encode_utf16().collect()); }
  }
  variables.sort_by_key(|value| String::from_utf16_lossy(value).to_uppercase());
  let mut block = Vec::new();
  for variable in variables { block.extend(variable); block.push(0); }
  block.push(0);
  if block.len() == 1 { block.push(0); }
  block
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn both_launch_paths_override_external_proxy_without_changing_parent() {
    let proxy = "http://127.0.0.1:8365";
    let mut command = Command::new("unused");
    command.env("HTTP_PROXY", "http://127.0.0.1:7890").env("ALL_PROXY", "socks5://localhost:7890").env("NO_PROXY", "*");
    apply(&mut command, Some(proxy), false);
    let configured: std::collections::HashMap<_, _> = command.get_envs().map(|(key, value)| (key.to_string_lossy().to_ascii_uppercase(), value.map(|v| v.to_string_lossy().into_owned()))).collect();
    assert_eq!(configured.get("HTTP_PROXY").unwrap().as_deref(), Some(proxy));
    assert_eq!(configured.get("HTTPS_PROXY").unwrap().as_deref(), Some(proxy));
    assert_eq!(configured.get("ALL_PROXY"), Some(&None));
    assert_eq!(configured.get("NO_PROXY"), Some(&None));
    let inherited: Vec<Vec<u16>> = ["http_proxy=http://127.0.0.1:7890", "Https_Proxy=http://127.0.0.1:7890", "ALL_PROXY=socks5://localhost:7890", "no_proxy=*", "PATH=C:\\Windows", "__COMPAT_LAYER=OLD"].iter().map(|s| s.encode_utf16().collect()).collect();
    let original = inherited.clone();
    let block = desktop_block(inherited, Some(proxy), true);
    let entries: Vec<_> = block.split(|c| *c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect();
    assert!(entries.contains(&format!("HTTP_PROXY={}", proxy)));
    assert!(entries.contains(&format!("HTTPS_PROXY={}", proxy)));
    assert!(entries.contains(&"__COMPAT_LAYER=RUNASINVOKER".into()));
    assert!(entries.contains(&"PATH=C:\\Windows".into()));
    assert!(!entries.iter().any(|s| s.contains("7890") || s.to_ascii_uppercase().starts_with("NO_PROXY=")));
    assert!(String::from_utf16_lossy(&original[0]).contains("7890"));
  }

  #[test]
  fn disabled_internal_proxy_preserves_environment() {
    let source: Vec<Vec<u16>> = ["HTTP_PROXY=http://localhost:7890", "NO_PROXY=localhost"].iter().map(|s| s.encode_utf16().collect()).collect();
    let block = desktop_block(source, None, false);
    assert!(String::from_utf16_lossy(&block).contains("7890"));
    assert!(String::from_utf16_lossy(&block).contains("NO_PROXY=localhost"));
  }
}
