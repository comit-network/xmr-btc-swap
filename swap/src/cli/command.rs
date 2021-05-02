use crate::fs::system_data_dir;
use anyhow::{Context, Result};
use libp2p::core::Multiaddr;
use libp2p::PeerId;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use url::Url;
use uuid::Uuid;

// Port is assumed to be stagenet standard port 38081
pub const DEFAULT_STAGENET_MONERO_DAEMON_HOST: &str = "monero-stagenet.exan.tech";

pub const DEFAULT_ELECTRUM_HTTP_URL: &str = "https://blockstream.info/testnet/api/";
const DEFAULT_ELECTRUM_RPC_URL: &str = "ssl://electrum.blockstream.info:60002";

const DEFAULT_TOR_SOCKS5_PORT: &str = "9050";

// Bitcoin transactions should be confirmed within X blocks
const DEFAUL_BITCOIN_CONFIRMATION_TARGET: &str = "3";

#[derive(structopt::StructOpt, Debug)]
#[structopt(name = "swap", about = "CLI for swapping BTC for XMR", author)]
pub struct Arguments {
    #[structopt(
        long = "--data-dir",
        help = "Provide the data directory path to be used to store application data",
        default_value
    )]
    pub data: Data,

    #[structopt(long, help = "Activate debug logging.")]
    pub debug: bool,

    #[structopt(subcommand)]
    pub cmd: Command,
}

#[derive(structopt::StructOpt, Debug)]
pub enum Command {
    /// Start a XMR for BTC swap
    BuyXmr {
        #[structopt(long = "seller-peer-id", help = "The seller's peer id")]
        alice_peer_id: PeerId,

        #[structopt(long = "seller-addr", help = "The seller's multiaddress")]
        alice_multiaddr: Multiaddr,

        #[structopt(long = "electrum-rpc",
        help = "Provide the Bitcoin Electrum RPC URL",
        default_value = DEFAULT_ELECTRUM_RPC_URL
        )]
        electrum_rpc_url: Url,

        #[structopt(flatten)]
        monero_params: MoneroParams,

        #[structopt(long = "tor-socks5-port", help = "Your local Tor socks5 proxy port", default_value = DEFAULT_TOR_SOCKS5_PORT)]
        tor_socks5_port: u16,

        #[structopt(long = "bitcoin-target-block", help = "Within how many blocks should the Bitcoin transactions be confirmed.", default_value = DEFAUL_BITCOIN_CONFIRMATION_TARGET)]
        bitcoin_target_block: usize,
    },
    /// Show a list of past ongoing and completed swaps
    History,
    /// Resume a swap
    Resume {
        #[structopt(
            long = "swap-id",
            help = "The swap id can be retrieved using the history subcommand"
        )]
        swap_id: Uuid,

        #[structopt(long = "seller-addr", help = "The seller's multiaddress")]
        alice_multiaddr: Multiaddr,

        #[structopt(long = "electrum-rpc",
        help = "Provide the Bitcoin Electrum RPC URL",
        default_value = DEFAULT_ELECTRUM_RPC_URL
        )]
        electrum_rpc_url: Url,

        #[structopt(flatten)]
        monero_params: MoneroParams,

        #[structopt(long = "tor-socks5-port", help = "Your local Tor socks5 proxy port", default_value = DEFAULT_TOR_SOCKS5_PORT)]
        tor_socks5_port: u16,

        #[structopt(long = "bitcoin-target-block", help = "Within how many blocks should the Bitcoin transactions be confirmed.", default_value = DEFAUL_BITCOIN_CONFIRMATION_TARGET)]
        bitcoin_target_block: usize,
    },
    /// Try to cancel an ongoing swap (expert users only)
    Cancel {
        #[structopt(
            long = "swap-id",
            help = "The swap id can be retrieved using the history subcommand"
        )]
        swap_id: Uuid,

        #[structopt(short, long)]
        force: bool,

        #[structopt(long = "electrum-rpc",
        help = "Provide the Bitcoin Electrum RPC URL",
        default_value = DEFAULT_ELECTRUM_RPC_URL
        )]
        electrum_rpc_url: Url,

        #[structopt(long = "bitcoin-target-block", help = "Within how many blocks should the Bitcoin transactions be confirmed.", default_value = DEFAUL_BITCOIN_CONFIRMATION_TARGET)]
        bitcoin_target_block: usize,
    },
    /// Try to cancel a swap and refund my BTC (expert users only)
    Refund {
        #[structopt(
            long = "swap-id",
            help = "The swap id can be retrieved using the history subcommand"
        )]
        swap_id: Uuid,

        #[structopt(short, long)]
        force: bool,

        #[structopt(long = "electrum-rpc",
        help = "Provide the Bitcoin Electrum RPC URL",
        default_value = DEFAULT_ELECTRUM_RPC_URL
        )]
        electrum_rpc_url: Url,

        #[structopt(long = "bitcoin-target-block", help = "Within how many blocks should the Bitcoin transactions be confirmed.", default_value = DEFAUL_BITCOIN_CONFIRMATION_TARGET)]
        bitcoin_target_block: usize,
    },
}

#[derive(structopt::StructOpt, Debug)]
pub struct MoneroParams {
    #[structopt(long = "receive-address",
        help = "Provide the monero address where you would like to receive monero",
        parse(try_from_str = parse_monero_address)
    )]
    pub receive_monero_address: monero::Address,

    #[structopt(
        long = "monero-daemon-host",
        help = "Specify to connect to a monero daemon of your choice",
        default_value = DEFAULT_STAGENET_MONERO_DAEMON_HOST
    )]
    pub monero_daemon_host: String,
}

#[derive(Clone, Debug)]
pub struct Data(pub PathBuf);

/// Default location for storing data for the CLI
// Takes the default system data-dir and adds a `/cli`
impl Default for Data {
    fn default() -> Self {
        Data(
            system_data_dir()
                .map(|proj_dir| Path::join(&proj_dir, "cli"))
                .expect("computed valid path for data dir"),
        )
    }
}

impl FromStr for Data {
    type Err = core::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Data(PathBuf::from_str(s)?))
    }
}

impl ToString for Data {
    fn to_string(&self) -> String {
        self.0
            .clone()
            .into_os_string()
            .into_string()
            .expect("default datadir to be convertible to string")
    }
}

fn parse_monero_address(s: &str) -> Result<monero::Address> {
    monero::Address::from_str(s).with_context(|| {
        format!(
            "Failed to parse {} as a monero address, please make sure it is a valid address",
            s
        )
    })
}
