use libp2p::{core::Multiaddr, PeerId};
use std::str::FromStr;
use url::Url;
use uuid::Uuid;

#[derive(structopt::StructOpt, Debug)]
#[structopt(name = "xmr-btc-swap", about = "Trustless XMR BTC swaps")]
pub enum Options {
    Alice {
        #[structopt(
            short = "b",
            long = "bitcoind",
            default_value = "http://127.0.0.1:8332"
        )]
        bitcoind_url: Url,

        #[structopt(short = "n", long = "bitcoin-wallet-name")]
        bitcoin_wallet_name: String,

        #[structopt(
            short = "m",
            long = "monero-wallet-rpc",
            default_value = "http://127.0.0.1:18083/json_rpc"
        )]
        monero_wallet_rpc_url: Url,

        #[structopt(
            short = "a",
            long = "listen-addr",
            default_value = "/ip4/127.0.0.1/tcp/9876"
        )]
        listen_addr: Multiaddr,

        #[structopt(short = "t", long = "tor-port")]
        tor_port: Option<u16>,

        #[structopt(short = "s", long = "send-piconeros", parse(try_from_str = parse_pics))]
        send_monero: xmr_btc::monero::Amount,

        #[structopt(short = "r", long = "receive-sats", parse(try_from_str = parse_sats))]
        receive_bitcoin: bitcoin::Amount,
    },
    Bob {
        #[structopt(short = "a", long = "alice-addr")]
        alice_addr: Multiaddr,

        #[structopt(short = "p", long = "alice-peer-id")]
        alice_peer_id: PeerId,

        #[structopt(
            short = "b",
            long = "bitcoind",
            default_value = "http://127.0.0.1:8332"
        )]
        bitcoind_url: Url,

        #[structopt(short = "n", long = "bitcoin-wallet-name")]
        bitcoin_wallet_name: String,

        #[structopt(
            short = "m",
            long = "monerod",
            default_value = "http://127.0.0.1:18083/json_rpc"
        )]
        monero_wallet_rpc_url: Url,

        #[structopt(short = "t", long = "tor")]
        tor: bool,

        #[structopt(short = "s", long = "send-sats", parse(try_from_str = parse_sats))]
        send_bitcoin: bitcoin::Amount,

        #[structopt(short = "r", long = "receive-piconeros", parse(try_from_str = parse_pics))]
        receive_monero: xmr_btc::monero::Amount,
    },
    History,
    Recover {
        #[structopt(required = true)]
        swap_id: Uuid,

        #[structopt(default_value = "http://127.0.0.1:8332", long = "bitcoind")]
        bitcoind_url: Url,

        #[structopt(default_value = "http://127.0.0.1:18083/json_rpc", long = "monerod")]
        monerod_url: Url,

        #[structopt(short = "n", long = "bitcoin-wallet-name")]
        bitcoin_wallet_name: String,
    },
}

fn parse_sats(str: &str) -> anyhow::Result<bitcoin::Amount> {
    let sats = u64::from_str(str)?;
    let amount = bitcoin::Amount::from_sat(sats);
    Ok(amount)
}

fn parse_pics(str: &str) -> anyhow::Result<xmr_btc::monero::Amount> {
    let pics = u64::from_str(str)?;
    let amount = xmr_btc::monero::Amount::from_piconero(pics);
    Ok(amount)
}
