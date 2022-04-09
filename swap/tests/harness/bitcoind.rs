use std::collections::HashMap;
use testcontainers::core::{Container, Docker, WaitForMessage};
use testcontainers::Image;

pub const RPC_USER: &str = "admin";
pub const RPC_PASSWORD: &str = "123";
pub const RPC_PORT: u16 = 18443;
pub const PORT: u16 = 18886;
pub const DATADIR: &str = "/home/bdk";

#[derive(Debug)]
pub struct Bitcoind {
    args: BitcoindArgs,
    entrypoint: Option<String>,
    volume: Option<String>,
}

impl Image for Bitcoind {
    type Args = BitcoindArgs;
    type EnvVars = HashMap<String, String>;
    type Volumes = HashMap<String, String>;
    type EntryPoint = str;

    fn descriptor(&self) -> String {
        "coblox/bitcoin-core:0.21.0".to_string()
    }

    fn wait_until_ready<D: Docker>(&self, container: &Container<'_, D, Self>) {
        container
            .logs()
            .stdout
            .wait_for_message("init message: Done loading")
            .unwrap();
    }

    fn args(&self) -> <Self as Image>::Args {
        self.args.clone()
    }

    fn volumes(&self) -> Self::Volumes {
        let mut volumes = HashMap::new();
        match self.volume.clone() {
            None => {}
            Some(volume) => {
                volumes.insert(volume, DATADIR.to_string());
            }
        }
        volumes
    }

    fn env_vars(&self) -> Self::EnvVars {
        HashMap::new()
    }

    fn with_args(self, args: <Self as Image>::Args) -> Self {
        Bitcoind { args, ..self }
    }

    fn with_entrypoint(self, entrypoint: &Self::EntryPoint) -> Self {
        Self {
            entrypoint: Some(entrypoint.to_string()),
            ..self
        }
    }

    fn entrypoint(&self) -> Option<String> {
        self.entrypoint.to_owned()
    }
}

impl Default for Bitcoind {
    fn default() -> Self {
        Bitcoind {
            args: BitcoindArgs::default(),
            entrypoint: Some("/usr/bin/bitcoind".into()),
            volume: None,
        }
    }
}

impl Bitcoind {
    pub fn with_volume(mut self, volume: String) -> Self {
        self.volume = Some(volume);
        self
    }
}

#[derive(Debug, Clone)]
pub struct BitcoindArgs;

impl Default for BitcoindArgs {
    fn default() -> Self {
        BitcoindArgs
    }
}

impl IntoIterator for BitcoindArgs {
    type Item = String;
    type IntoIter = ::std::vec::IntoIter<String>;

    fn into_iter(self) -> <Self as IntoIterator>::IntoIter {
        let args = vec![
            "-server".to_string(),
            "-regtest".to_string(),
            "-listen=1".to_string(),
            "-prune=0".to_string(),
            "-rpcallowip=0.0.0.0/0".to_string(),
            "-rpcbind=0.0.0.0".to_string(),
            format!("-rpcuser={}", RPC_USER),
            format!("-rpcpassword={}", RPC_PASSWORD),
            "-printtoconsole".to_string(),
            "-rest".to_string(),
            "-fallbackfee=0.0002".to_string(),
            format!("-datadir={}", DATADIR),
            format!("-rpcport={}", RPC_PORT),
            format!("-port={}", PORT),
            "-rest".to_string(),
        ];

        args.into_iter()
    }
}
