#[derive(structopt::StructOpt, Debug)]
pub struct Options {
    /// Run the swap as Alice.
    #[structopt(long = "as-alice")]
    pub as_alice: bool,

    /// Run the swap as Bob and try to swap this many XMR (in piconero).
    #[structopt(long = "picos")]
    pub piconeros: Option<u64>,

    /// Run the swap as Bob and try to swap this many BTC (in satoshi).
    #[structopt(long = "sats")]
    pub satoshis: Option<u64>,

    /// Alice's onion multitaddr (only required for Bob, Alice will autogenerate
    /// one)
    #[structopt(long)]
    pub alice_address: Option<String>,
}
