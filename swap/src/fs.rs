use anyhow::Context;
use directories_next::ProjectDirs;
use std::path::{Path, PathBuf};

/// This is to store the configuration and seed files
// Linux: /home/<user>/.config/xmr-btc-swap/
// OSX: /Users/<user>/Library/Preferences/xmr-btc-swap/
#[allow(dead_code)]
fn config_dir() -> Option<PathBuf> {
    ProjectDirs::from("", "", "xmr-btc-swap").map(|proj_dirs| proj_dirs.config_dir().to_path_buf())
}

#[allow(dead_code)]
pub fn default_config_path() -> anyhow::Result<PathBuf> {
    config_dir()
        .map(|dir| Path::join(&dir, "config.toml"))
        .context("Could not generate default configuration path")
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
