use libp2p::{core::Multiaddr, PeerId};
use url::Url;
use uuid::Uuid;

use crate::{bitcoin, monero};

#[derive(structopt::StructOpt, Debug)]
pub struct Options {
    // TODO: Default value should points to proper configuration folder in home folder
    #[structopt(long = "data-dir", default_value = "./.swap-data/")]
    pub data_dir: String,

    #[structopt(subcommand)]
    pub cmd: Command,
}

#[derive(structopt::StructOpt, Debug)]
#[structopt(name = "xmr_btc-swap", about = "XMR BTC atomic swap")]
pub enum Command {
    SellXmr {
        #[structopt(long = "bitcoind-rpc", default_value = "http://127.0.0.1:8332")]
        bitcoind_url: Url,

        #[structopt(long = "bitcoin-wallet-name")]
        bitcoin_wallet_name: String,

        #[structopt(
            long = "monero-wallet-rpc",
            default_value = "http://127.0.0.1:18083/json_rpc"
        )]
        monero_wallet_rpc_url: Url,

        #[structopt(long = "p2p-address", default_value = "/ip4/0.0.0.0/tcp/9876")]
        listen_addr: Multiaddr,

        #[structopt(long = "send-xmr",  help = "Monero amount as floating point nr without denomination (e.g. 125.1)", parse(try_from_str = parse_xmr))]
        send_monero: monero::Amount,

        #[structopt(long = "receive-btc", help = "Bitcoin amount as floating point nr without denomination (e.g. 1.25)", parse(try_from_str = parse_btc))]
        receive_bitcoin: bitcoin::Amount,
    },
    BuyXmr {
        #[structopt(long = "connect-peer-id")]
        alice_peer_id: PeerId,

        #[structopt(long = "connect-addr")]
        alice_addr: Multiaddr,

        #[structopt(long = "bitcoind-rpc", default_value = "http://127.0.0.1:8332")]
        bitcoind_url: Url,

        #[structopt(long = "bitcoin-wallet-name")]
        bitcoin_wallet_name: String,

        #[structopt(
            long = "monero-wallet-rpc",
            default_value = "http://127.0.0.1:18083/json_rpc"
        )]
        monero_wallet_rpc_url: Url,

        #[structopt(long = "send-btc", help = "Bitcoin amount as floating point nr without denomination (e.g. 1.25)", parse(try_from_str = parse_btc))]
        send_bitcoin: bitcoin::Amount,

        #[structopt(long = "receive-xmr", help = "Monero amount as floating point nr without denomination (e.g. 125.1)", parse(try_from_str = parse_xmr))]
        receive_monero: monero::Amount,
    },
    History,
    Resume(Resume),
}

#[derive(structopt::StructOpt, Debug)]
pub enum Resume {
    SellXmr {
        #[structopt(long = "swap-id")]
        swap_id: Uuid,

        #[structopt(long = "bitcoind-rpc", default_value = "http://127.0.0.1:8332")]
        bitcoind_url: Url,

        #[structopt(long = "bitcoin-wallet-name")]
        bitcoin_wallet_name: String,

        #[structopt(
            long = "monero-wallet-rpc",
            default_value = "http://127.0.0.1:18083/json_rpc"
        )]
        monero_wallet_rpc_url: Url,

        #[structopt(long = "listen-address", default_value = "/ip4/127.0.0.1/tcp/9876")]
        listen_addr: Multiaddr,
    },
    BuyXmr {
        #[structopt(long = "swap-id")]
        swap_id: Uuid,

        #[structopt(long = "counterpart-peer-id")]
        alice_peer_id: PeerId,

        #[structopt(long = "counterpart-addr")]
        alice_addr: Multiaddr,

        #[structopt(long = "bitcoind-rpc", default_value = "http://127.0.0.1:8332")]
        bitcoind_url: Url,

        #[structopt(long = "bitcoin-wallet-name")]
        bitcoin_wallet_name: String,

        #[structopt(
            long = "monero-wallet-rpc",
            default_value = "http://127.0.0.1:18083/json_rpc"
        )]
        monero_wallet_rpc_url: Url,
    },
}

fn parse_btc(str: &str) -> anyhow::Result<bitcoin::Amount> {
    let amount = bitcoin::Amount::from_str_in(str, ::bitcoin::Denomination::Bitcoin)?;
    Ok(amount)
}

fn parse_xmr(str: &str) -> anyhow::Result<monero::Amount> {
    let amount = monero::Amount::parse_monero(str)?;
    Ok(amount)
}
