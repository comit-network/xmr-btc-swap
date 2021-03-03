use anyhow::{Context, Result};
use libp2p::{core::Multiaddr, PeerId};
use std::{path::PathBuf, str::FromStr};
use uuid::Uuid;

pub const DEFAULT_ALICE_MULTIADDR: &str = "/dns4/xmr-btc-asb.coblox.tech/tcp/9876";
pub const DEFAULT_ALICE_PEER_ID: &str = "12D3KooWCdMKjesXMJz1SiZ7HgotrxuqhQJbP5sgBm2BwP1cqThi";

#[derive(structopt::StructOpt, Debug)]
pub struct Arguments {
    #[structopt(
        long = "config",
        help = "Provide a custom path to the configuration file. The configuration file must be a toml file.",
        parse(from_os_str)
    )]
    pub config: Option<PathBuf>,

    #[structopt(long, help = "Activate debug logging.")]
    pub debug: bool,

    #[structopt(subcommand)]
    pub cmd: Command,
}

#[derive(structopt::StructOpt, Debug)]
#[structopt(name = "xmr_btc-swap", about = "XMR BTC atomic swap")]
pub enum Command {
    BuyXmr {
        #[structopt(long = "receive-address", parse(try_from_str = parse_monero_address))]
        receive_monero_address: monero::Address,

        #[structopt(long = "connect-peer-id", default_value = DEFAULT_ALICE_PEER_ID)]
        alice_peer_id: PeerId,

        #[structopt(
        long = "connect-addr",
        default_value = DEFAULT_ALICE_MULTIADDR
        )]
        alice_addr: Multiaddr,
    },
    History,
    Resume {
        #[structopt(long = "receive-address", parse(try_from_str = parse_monero_address))]
        receive_monero_address: monero::Address,

        #[structopt(long = "swap-id")]
        swap_id: Uuid,

        // TODO: Remove Alice peer-id/address, it should be saved in the database when running swap
        // and loaded from the database when running resume/cancel/refund
        #[structopt(long = "counterpart-peer-id", default_value = DEFAULT_ALICE_PEER_ID)]
        alice_peer_id: PeerId,

        #[structopt(
        long = "counterpart-addr",
        default_value = DEFAULT_ALICE_MULTIADDR
        )]
        alice_addr: Multiaddr,
    },
    Cancel {
        #[structopt(long = "swap-id")]
        swap_id: Uuid,

        #[structopt(short, long)]
        force: bool,
    },
    Refund {
        #[structopt(long = "swap-id")]
        swap_id: Uuid,

        #[structopt(short, long)]
        force: bool,
    },
}

fn parse_monero_address(s: &str) -> Result<monero::Address> {
    monero::Address::from_str(s).with_context(|| {
        format!(
            "Failed to parse {} as a monero address, please make sure it is a valid address",
            s
        )
    })
}

#[cfg(test)]
mod tests {
    use crate::cli::command::{DEFAULT_ALICE_MULTIADDR, DEFAULT_ALICE_PEER_ID};
    use libp2p::{core::Multiaddr, PeerId};

    #[test]
    fn parse_default_alice_peer_id_success() {
        DEFAULT_ALICE_PEER_ID
            .parse::<PeerId>()
            .expect("default alice peer id str is a valid PeerId");
    }

    #[test]
    fn parse_default_alice_multiaddr_success() {
        DEFAULT_ALICE_MULTIADDR
            .parse::<Multiaddr>()
            .expect("default alice multiaddr str is a valid Multiaddr>");
    }
}
