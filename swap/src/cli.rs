use libp2p::core::Multiaddr;
use url::Url;
use uuid::Uuid;

// TODO: Remove monero_watch_only_wallet_rpc_url options.
//
// We need an extra `monero-wallet-rpc` to monitor the shared output without
// unloading the user's Monero wallet. A better approach than passing in an
// extra argument (and requiring the user to start up 2 `monero-wallet-rpc`
// instances), may be to start up another `monero-wallet-rpc` instance as
// part of executing this binary (i.e. requiring `monero-wallet-rpc` to be in
// the path).

#[derive(structopt::StructOpt, Debug)]
#[structopt(name = "xmr-btc-swap", about = "Trustless XMR BTC swaps")]
pub enum Options {
    Alice {
        #[structopt(default_value = "http://127.0.0.1:8332", long = "bitcoind")]
        bitcoind_url: Url,

        #[structopt(
            default_value = "http://127.0.0.1:18083/json_rpc",
            long = "monero_wallet_rpc"
        )]
        monero_wallet_rpc_url: Url,

        #[structopt(
            default_value = "http://127.0.0.1:18084",
            long = "monero_watch_only_wallet_rpc"
        )]
        monero_watch_only_wallet_rpc_url: Url,

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

        #[structopt(
            default_value = "http://127.0.0.1:18083/json_rpc",
            long = "monero_wallet_rpc"
        )]
        monero_wallet_rpc_url: Url,

        #[structopt(
            default_value = "http://127.0.0.1:18084",
            long = "monero_watch_only_wallet_rpc"
        )]
        monero_watch_only_wallet_rpc_url: Url,

        #[structopt(long = "tor")]
        tor: bool,
    },
    History,
    Recover {
        #[structopt(required = true)]
        swap_id: Uuid,

        #[structopt(default_value = "http://127.0.0.1:8332", long = "bitcoind")]
        bitcoind_url: Url,

        #[structopt(
            default_value = "http://127.0.0.1:18083/json_rpc",
            long = "monero_wallet_rpc"
        )]
        monero_wallet_rpc_url: Url,

        #[structopt(
            default_value = "http://127.0.0.1:18084",
            long = "monero_watch_only_wallet_rpc"
        )]
        monero_watch_only_wallet_rpc_url: Url,
    },
}
