use anyhow::Result;
use std::path::Path;

pub fn ensure_directory_exists(file: &Path) -> Result<(), std::io::Error> {
    if let Some(path) = file.parent() {
        if !path.exists() {
            tracing::info!(
                "Parent directory does not exist, creating recursively: {}",
                file.display()
            );
            return std::fs::create_dir_all(path);
        }
    }
    Ok(())
}
