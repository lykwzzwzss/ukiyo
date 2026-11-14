use std::path::PathBuf;

#[tauri::command]
pub fn dir_exists(path: &str) -> bool {
  std::fs::metadata(PathBuf::from(path)).is_ok()
}

#[tauri::command]
pub fn does_file_exist(path1: &str) -> bool {
  std::fs::metadata(path1).is_ok()
}

/// 比较两个文件内容是否完全一致（补丁是否已安装）。
pub fn are_files_identical(path1: &str, path2: &str) -> bool {
  match (std::fs::read(path1), std::fs::read(path2)) {
    (Ok(left), Ok(right)) => left == right,
    _ => false,
  }
}
