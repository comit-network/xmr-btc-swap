use anyhow::{Context, Result};
use directories_next::ProjectDirs;
use std::path::{Path, PathBuf};

/// This is the default location for the overall config-dir specific by system
// Linux: /home/<user>/.config/xmr-btc-swap/
// OSX: /Users/<user>/Library/Preferences/xmr-btc-swap/
pub fn system_config_dir() -> Result<PathBuf> {
    ProjectDirs::from("", "", "xmr-btc-swap")
        .map(|proj_dirs| proj_dirs.config_dir().to_path_buf())
        .context("Could not generate default system configuration dir path")
}

/// This is the default location for the overall data-dir specific by system
// Linux: /home/<user>/.local/share/xmr-btc-swap/
// OSX: /Users/<user>/Library/ApplicationSupport/xmr-btc-swap/
pub fn system_data_dir() -> Result<PathBuf> {
    ProjectDirs::from("", "", "xmr-btc-swap")
        .map(|proj_dirs| proj_dirs.data_dir().to_path_buf())
        .context("Could not generate default system data-dir dir path")
}

pub fn ensure_directory_exists(file: &Path) -> Result<(), std::io::Error> {
    if let Some(path) = file.parent() {
        if !path.exists() {
            tracing::info!(
                directory = %file.display(),
                "Parent directory does not exist, creating recursively",
            );
            return std::fs::create_dir_all(path);
        }
    }
    Ok(())
}
