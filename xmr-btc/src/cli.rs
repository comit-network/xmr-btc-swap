use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "xmr-btc", about = "A simple BTC/XMR atomic swap tool.")]
pub struct Opt {
    /// Run as the maker i.e., wait for a taker to connect and trade
    #[structopt(short, long)]
    pub maker: bool,

    /// Request a rate from a maker
    #[structopt(short, long)]
    pub taker: bool,

    /// Onion/Ipv4 mulitaddr of a maker
    #[structopt(long)]
    pub address: Option<String>,
}
