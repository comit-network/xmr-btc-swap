use crate::monero::Amount;
use anyhow::Result;
use std::path::PathBuf;

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
    Start {
        #[structopt(long = "max-sell-xmr", help = "The maximum amount of XMR the ASB is willing to sell.", default_value="0.5", parse(try_from_str = parse_xmr))]
        max_sell: Amount,
    },
    History,
}

fn parse_xmr(str: &str) -> Result<Amount> {
    let amount = Amount::parse_monero(str)?;
    Ok(amount)
}
