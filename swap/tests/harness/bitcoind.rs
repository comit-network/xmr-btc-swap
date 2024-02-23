use std::collections::BTreeMap;
use testcontainers::{core::WaitFor, Image, ImageArgs};

pub const RPC_USER: &str = "admin";
pub const RPC_PASSWORD: &str = "123";
pub const RPC_PORT: u16 = 18443;
pub const PORT: u16 = 18886;
pub const DATADIR: &str = "/home/bdk";

#[derive(Debug)]
pub struct Bitcoind {
    entrypoint: Option<String>,
    volumes: BTreeMap<String, String>,
}

impl Image for Bitcoind {
    type Args = BitcoindArgs;

    fn name(&self) -> String {
        "coblox/bitcoin-core".into()
    }

    fn tag(&self) -> String {
        "0.21.0".into()
    }

    fn ready_conditions(&self) -> Vec<WaitFor> {
        vec![WaitFor::message_on_stdout("init message: Done loading")]
    }

    fn volumes(&self) -> Box<dyn Iterator<Item = (&String, &String)> + '_> {
        Box::new(self.volumes.iter())
    }

    fn entrypoint(&self) -> Option<String> {
        self.entrypoint.to_owned()
    }
}

impl Default for Bitcoind {
    fn default() -> Self {
        Bitcoind {
            entrypoint: Some("/usr/bin/bitcoind".into()),
            volumes: BTreeMap::default(),
        }
    }
}

impl Bitcoind {
    pub fn with_volume(mut self, volume: String) -> Self {
        self.volumes.insert(volume, DATADIR.to_string());
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
            "-fallbackfee=0.0002".to_string(),
            format!("-datadir={}", DATADIR),
            format!("-rpcport={}", RPC_PORT),
            format!("-port={}", PORT),
            "-rest".to_string(),
        ];

        args.into_iter()
    }
}

impl ImageArgs for BitcoindArgs {
    fn into_iterator(self) -> Box<dyn Iterator<Item = String>> {
        Box::new(self.into_iter())
    }
}
