use crate::bitcoin::Amount;
use bitcoin::util::amount::ParseAmountError;
use bitcoin::Denomination;
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
        #[structopt(long = "max-buy-btc", help = "The maximum amount of BTC the ASB is willing to buy.", default_value="0.005", parse(try_from_str = parse_btc))]
        max_buy: Amount,
    },
    History,
}

fn parse_btc(s: &str) -> Result<Amount, ParseAmountError> {
    Amount::from_str_in(s, Denomination::Bitcoin)
}
