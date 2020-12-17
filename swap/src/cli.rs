use libp2p::core::Multiaddr;
use url::Url;
use uuid::Uuid;

#[derive(structopt::StructOpt, Debug)]
pub struct Options {
    // TODO: Default value should points to proper configuration folder in home folder
    #[structopt(short = "db", long = "database", default_value = "./.swap-db/")]
    pub db_path: String,

    #[structopt(subcommand)]
    pub cmd: Command,
}

#[derive(structopt::StructOpt, Debug)]
#[structopt(name = "xmr-btc-swap", about = "XMR BTC atomic swap")]
pub enum Command {
    SellXmr {
        #[structopt(
            short = "b",
            long = "bitcoind-rpc",
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
            long = "p2p-address",
            default_value = "/ip4/127.0.0.1/tcp/9876"
        )]
        listen_addr: Multiaddr,

        #[structopt(short = "s", long = "send-xmr",  help = "Monero amount as floating point nr without denomination (e.g. 125.1)", parse(try_from_str = parse_xmr))]
        send_monero: xmr_btc::monero::Amount,

        #[structopt(short = "r", long = "receive-btc", help = "Bitcoin amount as floating point nr without denomination (e.g. 1.25)", parse(try_from_str = parse_btc))]
        receive_bitcoin: bitcoin::Amount,
    },
    BuyXmr {
        #[structopt(short = "a", long = "connect-addr")]
        alice_addr: Multiaddr,

        #[structopt(
            short = "b",
            long = "bitcoind-rpc",
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

        #[structopt(short = "s", long = "send-btc", help = "Bitcoin amount as floating point nr without denomination (e.g. 1.25)", parse(try_from_str = parse_btc))]
        send_bitcoin: bitcoin::Amount,

        #[structopt(short = "r", long = "receive-xmr", help = "Monero amount as floating point nr without denomination (e.g. 125.1)", parse(try_from_str = parse_xmr))]
        receive_monero: xmr_btc::monero::Amount,
    },
    History,
    Resume {
        #[structopt(short = "id", long = "swap-id")]
        swap_id: Uuid,

        #[structopt(
            short = "b",
            long = "bitcoind-rpc",
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

        // TODO: The listen address is only relevant for Alice, but should be role independent
        //  see: https://github.com/comit-network/xmr-btc-swap/issues/77
        #[structopt(
            short = "a",
            long = "p2p-address",
            default_value = "/ip4/127.0.0.1/tcp/9876"
        )]
        listen_addr: Multiaddr,
    },
}

fn parse_btc(str: &str) -> anyhow::Result<bitcoin::Amount> {
    let amount = bitcoin::Amount::from_str_in(str, ::bitcoin::Denomination::Bitcoin)?;
    Ok(amount)
}

fn parse_xmr(str: &str) -> anyhow::Result<xmr_btc::monero::Amount> {
    let amount = xmr_btc::monero::Amount::parse_monero(str)?;
    Ok(amount)
}
