use libp2p::core::Multiaddr;
use url::Url;
use uuid::Uuid;

#[derive(structopt::StructOpt, Debug)]
#[structopt(name = "xmr-btc-swap", about = "Trustless XMR BTC swaps")]
pub enum Options {
    Alice {
        #[structopt(default_value = "http://127.0.0.1:8332", long = "bitcoind")]
        bitcoind_url: Url,

        #[structopt(default_value = "http://127.0.0.1:18083/json_rpc", long = "monerod")]
        monerod_url: Url,

        #[structopt(default_value = "/ip4/127.0.0.1/tcp/9876", long = "listen-addr")]
        listen_addr: Multiaddr,

        #[structopt(long = "tor-port")]
        tor_port: Option<u16>,
    },
    Bob {
        #[structopt(long = "sats")]
        satoshis: u64,

        #[structopt(long = "alice-addr")]
        alice_addr: Multiaddr,

        #[structopt(default_value = "http://127.0.0.1:8332", long = "bitcoind")]
        bitcoind_url: Url,

        #[structopt(default_value = "http://127.0.0.1:18083/json_rpc", long = "monerod")]
        monerod_url: Url,

        #[structopt(long = "tor")]
        tor: bool,
    },
    History,
    Recover {
        #[structopt(required = true)]
        swap_id: Uuid,

        #[structopt(default_value = "http://127.0.0.1:8332", long = "bitcoind")]
        bitcoind_url: Url,

        #[structopt(default_value = "http://127.0.0.1:18083/json_rpc", long = "monerod")]
        monerod_url: Url,
    },
}
