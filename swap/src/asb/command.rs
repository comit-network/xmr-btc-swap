use crate::bitcoin::Amount;
use bitcoin::Address;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(structopt::StructOpt, Debug)]
#[structopt(
    name = "asb",
    about = "Automated Swap Backend for swapping XMR for BTC",
    author
)]
pub struct Arguments {
    #[structopt(long, help = "Swap on testnet")]
    pub testnet: bool,

    #[structopt(
        short,
        long = "json",
        help = "Changes the log messages to json vs plain-text. If you run ASB as a service, it is recommended to set this to true to simplify log analyses."
    )]
    pub json: bool,

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
    #[structopt(about = "Main command to run the ASB.")]
    Start {
        #[structopt(
            long = "resume-only",
            help = "For maintenance only. When set, no new swap requests will be accepted, but existing unfinished swaps will be resumed."
        )]
        resume_only: bool,
    },
    #[structopt(about = "Prints swap-id and the state of each swap ever made.")]
    History,
    #[structopt(about = "Allows withdrawing BTC from the internal Bitcoin wallet.")]
    WithdrawBtc {
        #[structopt(
            long = "amount",
            help = "Optionally specify the amount of Bitcoin to be withdrawn. If not specified the wallet will be drained."
        )]
        amount: Option<Amount>,
        #[structopt(long = "address", help = "The address to receive the Bitcoin.")]
        address: Address,
    },
    #[structopt(
        about = "Prints the Bitcoin and Monero balance. Requires the monero-wallet-rpc to be running."
    )]
    Balance,
    #[structopt(about = "Contains sub-commands for recovering a swap manually.")]
    ManualRecovery(ManualRecovery),
}

#[derive(structopt::StructOpt, Debug)]
pub enum ManualRecovery {
    #[structopt(
        about = "Publishes the Bitcoin redeem transaction. This requires that we learned the encrypted signature from Bob and is only safe if no timelock has expired."
    )]
    Redeem {
        #[structopt(flatten)]
        redeem_params: RecoverCommandParams,

        #[structopt(
            long = "do_not_await_finality",
            help = "If this flag is present we exit directly after publishing the redeem transaction without waiting for the transaction to be included in a block"
        )]
        do_not_await_finality: bool,
    },
    #[structopt(
        about = "Publishes the Bitcoin cancel transaction. By default, the cancel timelock will be enforced. A confirmed cancel transaction enables refund and punish."
    )]
    Cancel {
        #[structopt(flatten)]
        cancel_params: RecoverCommandParams,
    },
    #[structopt(
        about = "Publishes the Monero refund transaction. By default, a swap-state where the cancel transaction was already published will be enforced. This command requires the counterparty Bitcoin refund transaction and will error if it was not published yet. "
    )]
    Refund {
        #[structopt(flatten)]
        refund_params: RecoverCommandParams,
    },
    #[structopt(
        about = "Publishes the Bitcoin punish transaction. By default, the punish timelock and a swap-state where the cancel transaction was already published will be enforced."
    )]
    Punish {
        #[structopt(flatten)]
        punish_params: RecoverCommandParams,
    },
    #[structopt(about = "Safely Abort requires the swap to be in a state prior to locking XMR.")]
    SafelyAbort {
        #[structopt(
            long = "swap-id",
            help = "The swap id can be retrieved using the history subcommand"
        )]
        swap_id: Uuid,
    },
}

#[derive(structopt::StructOpt, Debug)]
pub struct RecoverCommandParams {
    #[structopt(
        long = "swap-id",
        help = "The swap id can be retrieved using the history subcommand"
    )]
    pub swap_id: Uuid,

    #[structopt(
        short,
        long,
        help = "Circumvents certain checks when recovering. It is recommended to run a recovery command without --force first to see what is returned."
    )]
    pub force: bool,
}
