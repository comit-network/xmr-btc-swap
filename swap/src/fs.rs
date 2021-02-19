use anyhow::{Context, Result};
use directories_next::ProjectDirs;
use std::path::{Path, PathBuf};

/// This is to store the configuration and seed files
// Linux: /home/<user>/.config/xmr-btc-swap/
// OSX: /Users/<user>/Library/Preferences/xmr-btc-swap/
fn default_config_dir() -> Option<PathBuf> {
    ProjectDirs::from("", "", "xmr-btc-swap").map(|proj_dirs| proj_dirs.config_dir().to_path_buf())
}

pub fn default_config_path() -> Result<PathBuf> {
    default_config_dir()
        .map(|dir| Path::join(&dir, "config.toml"))
        .context("Could not generate default configuration path")
}

/// This is to store the DB
// Linux: /home/<user>/.local/share/xmr-btc-swap/
// OSX: /Users/<user>/Library/Application Support/xmr-btc-swap/
pub fn default_data_dir() -> Option<std::path::PathBuf> {
    ProjectDirs::from("", "", "xmr-btc-swap").map(|proj_dirs| proj_dirs.data_dir().to_path_buf())
}

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
