use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub data_dir: PathBuf,
}

impl Config {
    pub fn new_with_port(host: String, port: u16, data_dir: PathBuf) -> Self {
        Self {
            host,
            port,
            data_dir,
        }
    }

    pub fn new_random_port(host: String, data_dir: PathBuf) -> Self {
        Self {
            host,
            port: 0,
            data_dir,
        }
    }
}
