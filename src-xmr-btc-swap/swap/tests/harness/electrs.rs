use std::collections::BTreeMap;

use crate::harness::bitcoind;
use bitcoin::Network;
use testcontainers::{core::WaitFor, Image, ImageArgs};

pub const HTTP_PORT: u16 = 60401;
pub const RPC_PORT: u16 = 3002;

#[derive(Debug)]
pub struct Electrs {
    tag: String,
    args: ElectrsArgs,
    entrypoint: Option<String>,
    wait_for_message: String,
    volumes: BTreeMap<String, String>,
}

impl Image for Electrs {
    type Args = ElectrsArgs;
    fn name(&self) -> String {
        "vulpemventures/electrs".into()
    }

    fn tag(&self) -> String {
        self.tag.clone()
    }

    fn ready_conditions(&self) -> Vec<WaitFor> {
        vec![WaitFor::message_on_stderr(self.wait_for_message.clone())]
    }

    fn volumes(&self) -> Box<dyn Iterator<Item = (&String, &String)> + '_> {
        Box::new(self.volumes.iter())
    }

    fn entrypoint(&self) -> Option<String> {
        self.entrypoint.to_owned()
    }
}

impl Default for Electrs {
    fn default() -> Self {
        Electrs {
            tag: "v0.16.0.3".into(),
            args: ElectrsArgs::default(),
            entrypoint: Some("/build/electrs".into()),
            wait_for_message: "Running accept thread".to_string(),
            volumes: BTreeMap::default(),
        }
    }
}

impl Electrs {
    pub fn with_tag(self, tag_str: &str) -> Self {
        Electrs {
            tag: tag_str.to_string(),
            ..self
        }
    }

    pub fn with_volume(mut self, volume: String) -> Self {
        self.volumes.insert(volume, bitcoind::DATADIR.to_string());
        self
    }

    pub fn with_daemon_rpc_addr(mut self, name: String) -> Self {
        self.args.daemon_rpc_addr = name;
        self
    }

    pub fn self_and_args(self) -> (Self, ElectrsArgs) {
        let args = self.args.clone();
        (self, args)
    }
}

#[derive(Debug, Clone)]
pub struct ElectrsArgs {
    pub network: Network,
    pub daemon_dir: String,
    pub daemon_rpc_addr: String,
    pub cookie: String,
    pub http_addr: String,
    pub electrum_rpc_addr: String,
    pub cors: String,
}

impl Default for ElectrsArgs {
    fn default() -> Self {
        // todo: these "defaults" are only suitable for our tests and need to be looked
        // at
        ElectrsArgs {
            network: Network::Regtest,
            daemon_dir: bitcoind::DATADIR.to_string(),
            daemon_rpc_addr: format!("0.0.0.0:{}", bitcoind::RPC_PORT),
            cookie: format!("{}:{}", bitcoind::RPC_USER, bitcoind::RPC_PASSWORD),
            http_addr: format!("0.0.0.0:{}", HTTP_PORT),
            electrum_rpc_addr: format!("0.0.0.0:{}", RPC_PORT),
            cors: "*".to_string(),
        }
    }
}

impl IntoIterator for ElectrsArgs {
    type Item = String;
    type IntoIter = ::std::vec::IntoIter<String>;

    fn into_iter(self) -> <Self as IntoIterator>::IntoIter {
        let mut args = Vec::new();

        match self.network {
            Network::Testnet => args.push("--network=testnet".to_string()),
            Network::Regtest => args.push("--network=regtest".to_string()),
            Network::Bitcoin => {}
            Network::Signet => panic!("signet not yet supported"),
        }

        args.push("-vvvvv".to_string());
        args.push(format!("--daemon-dir={}", self.daemon_dir.as_str()));
        args.push(format!("--daemon-rpc-addr={}", self.daemon_rpc_addr));
        args.push(format!("--cookie={}", self.cookie));
        args.push(format!("--http-addr={}", self.http_addr));
        args.push(format!("--electrum-rpc-addr={}", self.electrum_rpc_addr));
        args.push(format!("--cors={}", self.cors));

        args.into_iter()
    }
}

impl ImageArgs for ElectrsArgs {
    fn into_iterator(self) -> Box<dyn Iterator<Item = String>> {
        Box::new(self.into_iter())
    }
}
