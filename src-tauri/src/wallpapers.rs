use std::path::Path;

fn supported(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()).map(|extension| {
        matches!(extension.to_ascii_lowercase().as_str(),
            "jpg" | "jpeg" | "jfif" | "png" | "webp" | "gif" | "bmp" | "avif")
    }).unwrap_or(false)
}

fn scan(directory: &Path) -> Result<Vec<String>, String> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("Cannot read wallpaper directory: {}", error)),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry.file_type().map_err(|error| error.to_string())?.is_file() && supported(&entry.path()) {
            paths.push(entry.path().to_string_lossy().into_owned());
        }
    }
    paths.sort();
    Ok(paths)
}

#[tauri::command]
pub fn list_wallpapers(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let directory = app.path_resolver().resource_dir()
        .ok_or_else(|| "Cannot locate wallpaper resources".to_string())?;
    scan(&directory.join("assets").join("wallpapers"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_common_formats_and_arbitrary_names() {
        for name in ["浮世 春.PNG", "a.b.JPEG", "7.jfif", "image.webp", "movie.GIF", "1.bmp", "a.avif", "a.jpg"] {
            assert!(supported(Path::new(name)), "{}", name);
        }
        for name in ["notes.txt", "image.jpg.exe", "image", "archive.zip"] {
            assert!(!supported(Path::new(name)), "{}", name);
        }
    }

    #[test]
    fn missing_directory_is_an_empty_playlist() {
        assert!(scan(&Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/nonexistent-wallpaper-test")).unwrap().is_empty());
    }
}
