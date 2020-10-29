use libp2p::core::Multiaddr;
use url::Url;

#[derive(structopt::StructOpt, Debug)]
pub enum Options {
    Alice {
        /// Run the swap as Bob and try to swap this many BTC (in satoshi).
        #[structopt(long = "sats")]
        satoshis: u64,

        // /// Run the swap as Bob and try to swap this many XMR (in piconero).
        // #[structopt(long = "picos", conflicts_with = "sats"))]
        // pub piconeros: u64,
        #[structopt(default_value = "http://127.0.0.1:8332", long = "bitcoind")]
        bitcoind_url: Url,

        #[structopt(default_value = "127.0.0.1", long = "listen_addr")]
        listen_addr: String,

        #[structopt(default_value = 9876, long = "list_port")]
        listen_port: u16,
    },
    Bob {
        /// Alice's multitaddr (only required for Bob, Alice will autogenerate
        /// one)
        #[structopt(long = "alice_addr")]
        alice_addr: Multiaddr,

        /// Run the swap as Bob and try to swap this many BTC (in satoshi).
        #[structopt(long = "sats")]
        satoshis: u64,

        // /// Run the swap as Bob and try to swap this many XMR (in piconero).
        // #[structopt(long = "picos", conflicts_with = "sats"))]
        // pub piconeros: u64,
        #[structopt(default_value = "http://127.0.0.1:8332", long = "bitcoind")]
        bitcoind_url: Url,
        // #[structopt(default_value = "/ip4/127.0.0.1/tcp/9876", long = "dial")]
        // alice_addr: String,
        #[cfg(feature = "tor")]
        #[structopt(long = "tor")]
        tor_port: u16,
    },
}
