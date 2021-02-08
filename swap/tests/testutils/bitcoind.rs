use bitcoin::Network;
use std::collections::HashMap;
use testcontainers::{
    core::{Container, Docker, Port, WaitForMessage},
    Image,
};

#[derive(Debug)]
pub struct Bitcoind {
    tag: String,
    args: BitcoindArgs,
    ports: Option<Vec<Port>>,
    entrypoint: Option<String>,
    volume: String,
}

impl Image for Bitcoind {
    type Args = BitcoindArgs;
    type EnvVars = HashMap<String, String>;
    type Volumes = HashMap<String, String>;
    type EntryPoint = str;

    fn descriptor(&self) -> String {
        format!("coblox/bitcoin-core:{}", self.tag)
    }

    fn wait_until_ready<D: Docker>(&self, container: &Container<'_, D, Self>) {
        container
            .logs()
            .stdout
            .wait_for_message(&"init message: Done loading")
            .unwrap();
    }

    fn args(&self) -> <Self as Image>::Args {
        self.args.clone()
    }

    fn volumes(&self) -> Self::Volumes {
        let mut volumes = HashMap::new();
        volumes.insert(self.volume.clone(), "/home/bdk-test".to_owned());
        volumes
    }

    fn env_vars(&self) -> Self::EnvVars {
        HashMap::new()
    }

    fn ports(&self) -> Option<Vec<Port>> {
        self.ports.clone()
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
            tag: "v0.19.1".into(),
            args: BitcoindArgs::default(),
            ports: None,
            entrypoint: Some("/usr/bin/bitcoind".into()),
            volume: uuid::Uuid::new_v4().to_string(),
        }
    }
}

impl Bitcoind {
    pub fn with_tag(self, tag_str: &str) -> Self {
        Bitcoind {
            tag: tag_str.to_string(),
            ..self
        }
    }

    pub fn with_mapped_port<P: Into<Port>>(mut self, port: P) -> Self {
        let mut ports = self.ports.unwrap_or_default();
        ports.push(port.into());
        self.ports = Some(ports);
        self
    }

    pub fn with_volume(mut self, volume: String) -> Self {
        self.volume = volume;
        self
    }
}

#[derive(Debug, Clone)]
pub struct BitcoindArgs;

/// Sane defaults for a mainnet regtest instance.
impl Default for BitcoindArgs {
    fn default() -> Self {
        BitcoindArgs
    }
}

impl IntoIterator for BitcoindArgs {
    type Item = String;
    type IntoIter = ::std::vec::IntoIter<String>;

    fn into_iter(self) -> <Self as IntoIterator>::IntoIter {
        let mut args = Vec::new();

        args.push("-server".to_string());
        args.push("-regtest".to_string());
        args.push("-txindex=1".to_string());
        args.push("-listen=1".to_string());
        args.push("-prune=0".to_string());
        args.push("-rpcallowip=0.0.0.0/0".to_string());
        args.push("-rpcbind=0.0.0.0".to_string());
        args.push("-rpcuser=admin".to_string());
        args.push("-rpcpassword=123".to_string());
        args.push("-printtoconsole".to_string());
        args.push("-rest".to_string());
        args.push("-fallbackfee=0.0002".to_string());
        args.push("-datadir=/home/bdk-test".to_string());
        args.push("-rpcport=18443".to_string());
        args.push("-port=18886".to_string());
        args.push("-rest".to_string());


        args.into_iter()
    }
}
