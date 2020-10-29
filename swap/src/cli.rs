use libp2p::core::Multiaddr;
use url::Url;

#[derive(structopt::StructOpt, Debug)]
pub enum Options {
    Alice {
        // /// Run the swap as Bob and try to swap this many XMR (in piconero).
        // #[structopt(long = "picos", conflicts_with = "sats"))]
        // pub piconeros: u64,
        #[structopt(default_value = "http://127.0.0.1:8332", long = "bitcoind")]
        bitcoind_url: Url,

        #[structopt(default_value = "/ip4/127.0.0.1/tcp/9876", long = "listen-addr")]
        listen_addr: Multiaddr,

        /// Run the swap as Bob and try to swap this many BTC (in satoshi).
        // #[cfg(feature = "tor")]
        #[structopt(long = "tor-port")]
        tor_port: Option<u16>,
    },
    Bob {
        /// Run the swap as Bob and try to swap this many BTC (in satoshi).
        #[structopt(long = "sats")]
        satoshis: u64,

        #[structopt(long = "alice-addr")]
        alice_addr: Multiaddr,

        // /// Run the swap as Bob and try to swap this many XMR (in piconero).
        // #[structopt(long = "picos", conflicts_with = "sats"))]
        // pub piconeros: u64,
        #[structopt(default_value = "http://127.0.0.1:8332", long = "bitcoind")]
        bitcoind_url: Url,
    },
}
