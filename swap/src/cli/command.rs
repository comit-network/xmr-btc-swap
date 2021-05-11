use crate::fs::system_data_dir;
use anyhow::{Context, Result};
use libp2p::core::Multiaddr;
use libp2p::PeerId;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use url::Url;
use uuid::Uuid;

pub const DEFAULT_STAGENET_MONERO_DAEMON_ADDRESS: &str = "monero-stagenet.exan.tech:38081";

const DEFAULT_ELECTRUM_RPC_URL: &str = "ssl://electrum.blockstream.info:60002";
const DEFAULT_BITCOIN_CONFIRMATION_TARGET: &str = "3";

const DEFAULT_TOR_SOCKS5_PORT: &str = "9050";

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
        seller_peer_id: PeerId,

        #[structopt(flatten)]
        seller_addr: SellerAddr,

        #[structopt(flatten)]
        bitcoin: Bitcoin,

        #[structopt(flatten)]
        monero: Monero,

        #[structopt(flatten)]
        tor: Tor,
    },
    /// Show a list of past ongoing and completed swaps
    History,
    /// Resume a swap
    Resume {
        #[structopt(flatten)]
        swap_id: SwapId,

        #[structopt(flatten)]
        seller_addr: SellerAddr,

        #[structopt(flatten)]
        bitcoin: Bitcoin,

        #[structopt(flatten)]
        monero: Monero,

        #[structopt(flatten)]
        tor: Tor,
    },
    /// Try to cancel an ongoing swap (expert users only)
    Cancel {
        #[structopt(flatten)]
        swap_id: SwapId,

        #[structopt(short, long)]
        force: bool,

        #[structopt(flatten)]
        bitcoin: Bitcoin,
    },
    /// Try to cancel a swap and refund my BTC (expert users only)
    Refund {
        #[structopt(flatten)]
        swap_id: SwapId,

        #[structopt(short, long)]
        force: bool,

        #[structopt(flatten)]
        bitcoin: Bitcoin,
    },
}

#[derive(structopt::StructOpt, Debug)]
pub struct Monero {
    #[structopt(long = "receive-address",
        help = "Provide the monero address where you would like to receive monero",
        parse(try_from_str = parse_monero_address)
    )]
    pub receive_monero_address: monero::Address,

    #[structopt(
        long = "monero-daemon-address",
        help = "Specify to connect to a monero daemon of your choice: <host>:<port>",
        default_value = DEFAULT_STAGENET_MONERO_DAEMON_ADDRESS
    )]
    pub monero_daemon_address: String,
}

#[derive(structopt::StructOpt, Debug)]
pub struct Bitcoin {
    #[structopt(long = "electrum-rpc",
    help = "Provide the Bitcoin Electrum RPC URL",
    default_value = DEFAULT_ELECTRUM_RPC_URL
    )]
    pub electrum_rpc_url: Url,

    #[structopt(long = "bitcoin-target-block", help = "Within how many blocks should the Bitcoin transactions be confirmed.", default_value = DEFAULT_BITCOIN_CONFIRMATION_TARGET)]
    pub bitcoin_target_block: usize,
}

#[derive(structopt::StructOpt, Debug)]
pub struct Tor {
    #[structopt(long = "tor-socks5-port", help = "Your local Tor socks5 proxy port", default_value = DEFAULT_TOR_SOCKS5_PORT)]
    pub tor_socks5_port: u16,
}

#[derive(structopt::StructOpt, Debug)]
pub struct SwapId {
    #[structopt(
        long = "swap-id",
        help = "The swap id can be retrieved using the history subcommand"
    )]
    pub swap_id: Uuid,
}

#[derive(structopt::StructOpt, Debug)]
pub struct SellerAddr {
    #[structopt(long = "seller-addr", help = "The seller's multiaddress")]
    pub seller_addr: Multiaddr,
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
