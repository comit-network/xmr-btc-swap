use crate::bitcoin::Amount;
use bitcoin::util::amount::ParseAmountError;
use bitcoin::{Address, Denomination};
use rust_decimal::Decimal;
use std::path::PathBuf;

#[derive(structopt::StructOpt, Debug)]
#[structopt(
    name = "asb",
    about = "Automated Swap Backend for swapping XMR for BTC",
    author
)]
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
        #[structopt(
            long = "ask-spread",
            help = "The spread in percent that should be applied to the asking price.",
            default_value = "0.02"
        )]
        ask_spread: Decimal,
    },
    History,
    WithdrawBtc {
        #[structopt(
            long = "amount",
            help = "Optionally specify the amount of Bitcoin to be withdrawn. If not specified the wallet will be drained."
        )]
        amount: Option<Amount>,
        #[structopt(long = "address", help = "The address to receive the Bitcoin.")]
        address: Address,
    },
    Balance,
}

fn parse_btc(s: &str) -> Result<Amount, ParseAmountError> {
    Amount::from_str_in(s, Denomination::Bitcoin)
}
