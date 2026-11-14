use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn decode_ini(bytes: &[u8]) -> Result<String, String> {
  if let Ok(text) = std::str::from_utf8(bytes) {
    return Ok(text.trim_start_matches('\u{feff}').to_string());
  }
  let (encoding, skip) = if bytes.starts_with(&[0xff, 0xfe]) { (encoding_rs::UTF_16LE, 2) }
    else if bytes.starts_with(&[0xfe, 0xff]) { (encoding_rs::UTF_16BE, 2) }
    else { (encoding_rs::GBK, 0) };
  let (text, errors) = encoding.decode_without_bom_handling(&bytes[skip..]);
  if errors { return Err("模组 INI 文本编码无效，请转为 UTF-8 后重试".into()); }
  Ok(text.into_owned())
}

pub fn read_ini(path: &Path) -> Result<String, String> {
  let bytes = std::fs::read(path).map_err(|error| format!("无法读取 {}：{}", path.display(), error))?;
  decode_ini(&bytes).map_err(|error| format!("{}：{}", path.display(), error))
}

// Readers (including the game) must never see a partially written INI.
pub fn write_changed(path: &Path, bytes: impl AsRef<[u8]>) -> Result<(), String> {
  let bytes = bytes.as_ref();
  if std::fs::read(path).map_or(false, |existing| existing == bytes) {
    return Ok(());
  }
  let parent = path.parent().ok_or("无效的配置路径")?;
  std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
  let temporary = path.with_extension(format!("tmp-{}-{}", std::process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed)));
  let result = std::fs::write(&temporary, bytes).and_then(|_| std::fs::rename(&temporary, path));
  if result.is_err() { let _ = std::fs::remove_file(&temporary); }
  result.map_err(|error| format!("无法写入 {}：{}", path.display(), error))
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn preserves_utf8_utf16_and_gbk_resource_names() {
    let text = "[Resource]\nfilename = 贴图.dds\n";
    assert_eq!(decode_ini(text.as_bytes()).unwrap(), text);
    let mut utf16 = vec![0xff, 0xfe];
    utf16.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    assert_eq!(decode_ini(&utf16).unwrap(), text);
    let (bytes, _, errors) = encoding_rs::GBK.encode(text);
    assert!(!errors);
    assert_eq!(decode_ini(&bytes).unwrap(), text);
  }
}
