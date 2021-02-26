use crate::bitcoin;
use anyhow::Result;
use libp2p::{core::Multiaddr, PeerId};
use std::path::PathBuf;
use uuid::Uuid;

const DEFAULT_ALICE_MULTIADDR: &str = "/dns4/xmr-btc-asb.coblox.tech/tcp/9876";
const DEFAULT_ALICE_PEER_ID: &str = "12D3KooWCdMKjesXMJz1SiZ7HgotrxuqhQJbP5sgBm2BwP1cqThi";

#[derive(structopt::StructOpt, Debug)]
pub struct Arguments {
    #[structopt(
        long = "config",
        help = "Provide a custom path to the configuration file. The configuration file must be a toml file.",
        parse(from_os_str)
    )]
    pub config: Option<PathBuf>,

    #[structopt(subcommand)]
    pub cmd: Command,
}

#[derive(structopt::StructOpt, Debug)]
#[structopt(name = "xmr_btc-swap", about = "XMR BTC atomic swap")]
pub enum Command {
    BuyXmr {
        #[structopt(long = "connect-peer-id", default_value = DEFAULT_ALICE_PEER_ID)]
        alice_peer_id: PeerId,

        #[structopt(
            long = "connect-addr",
            default_value = DEFAULT_ALICE_MULTIADDR
        )]
        alice_addr: Multiaddr,

        #[structopt(long = "send-btc", help = "Bitcoin amount as floating point nr without denomination (e.g. 1.25)", parse(try_from_str = parse_btc))]
        send_bitcoin: bitcoin::Amount,
    },
    History,
    Resume(Resume),
    Cancel(Cancel),
    Refund(Refund),
}

#[derive(structopt::StructOpt, Debug)]
pub enum Resume {
    BuyXmr {
        #[structopt(long = "swap-id")]
        swap_id: Uuid,

        #[structopt(long = "counterpart-peer-id", default_value = DEFAULT_ALICE_PEER_ID)]
        alice_peer_id: PeerId,

        #[structopt(
            long = "counterpart-addr",
            default_value = DEFAULT_ALICE_MULTIADDR
        )]
        alice_addr: Multiaddr,
    },
}

#[derive(structopt::StructOpt, Debug)]
pub enum Cancel {
    BuyXmr {
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

        #[structopt(short, long)]
        force: bool,
    },
}

#[derive(structopt::StructOpt, Debug)]
pub enum Refund {
    BuyXmr {
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

        #[structopt(short, long)]
        force: bool,
    },
}

fn parse_btc(str: &str) -> Result<bitcoin::Amount> {
    let amount = bitcoin::Amount::from_str_in(str, ::bitcoin::Denomination::Bitcoin)?;
    Ok(amount)
}
