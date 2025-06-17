use crate::asb::config::GetDefaults;
use crate::bitcoin::{bitcoin_address, Amount};
use crate::env;
use crate::env::GetConfig;
use anyhow::Result;
use bitcoin::address::NetworkUnchecked;
use bitcoin::Address;
use serde::Serialize;
use std::ffi::OsString;
use std::path::PathBuf;
use structopt::StructOpt;
use uuid::Uuid;

pub fn parse_args<I, T>(raw_args: I) -> Result<Arguments>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let matches = RawArguments::clap().get_matches_from_safe(raw_args)?;
    let args = RawArguments::from_clap(&matches);

    let json = args.json;
    let trace = args.trace;
    let testnet = args.testnet;
    let config = args.config;
    let command: RawCommand = args.cmd;

    let arguments = match command {
        RawCommand::Start { resume_only } => Arguments {
            testnet,
            json,
            trace,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::Start { resume_only },
        },
        RawCommand::History { only_unfinished } => Arguments {
            testnet,
            json,
            trace,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::History { only_unfinished },
        },
        RawCommand::Logs {
            logs_dir: dir_path,
            swap_id,
            redact,
        } => Arguments {
            testnet,
            json,
            trace,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::Logs {
                logs_dir: dir_path,
                swap_id,
                redact,
            },
        },
        RawCommand::WithdrawBtc { amount, address } => Arguments {
            testnet,
            json,
            trace,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::WithdrawBtc {
                amount,
                address: bitcoin_address::validate(address, testnet)?,
            },
        },
        RawCommand::Balance => Arguments {
            testnet,
            json,
            trace,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::Balance,
        },
        RawCommand::Config => Arguments {
            testnet,
            json,
            trace,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::Config,
        },
        RawCommand::ExportBitcoinWallet => Arguments {
            testnet,
            json,
            trace,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::ExportBitcoinWallet,
        },
        RawCommand::ExportMoneroWallet => Arguments {
            testnet,
            json,
            trace,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::ExportMoneroWallet,
        },
        RawCommand::ManualRecovery(ManualRecovery::Redeem {
            redeem_params: RecoverCommandParams { swap_id },
            do_not_await_finality,
        }) => Arguments {
            testnet,
            json,
            trace,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::Redeem {
                swap_id,

                do_not_await_finality,
            },
        },
        RawCommand::ManualRecovery(ManualRecovery::Cancel {
            cancel_params: RecoverCommandParams { swap_id },
        }) => Arguments {
            testnet,
            json,
            trace,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::Cancel { swap_id },
        },
        RawCommand::ManualRecovery(ManualRecovery::Refund {
            refund_params: RecoverCommandParams { swap_id },
        }) => Arguments {
            testnet,
            json,
            trace,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::Refund { swap_id },
        },
        RawCommand::ManualRecovery(ManualRecovery::Punish {
            punish_params: RecoverCommandParams { swap_id },
        }) => Arguments {
            testnet,
            json,
            trace,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::Punish { swap_id },
        },
        RawCommand::ManualRecovery(ManualRecovery::SafelyAbort { swap_id }) => Arguments {
            testnet,
            json,
            trace,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::SafelyAbort { swap_id },
        },
    };

    Ok(arguments)
}

fn config_path(config: Option<PathBuf>, is_testnet: bool) -> Result<PathBuf> {
    let config_path = if let Some(config_path) = config {
        config_path
    } else if is_testnet {
        env::Testnet::getConfigFileDefaults()?.config_path
    } else {
        env::Mainnet::getConfigFileDefaults()?.config_path
    };

    Ok(config_path)
}

fn env_config(is_testnet: bool) -> env::Config {
    if is_testnet {
        env::Testnet::get_config()
    } else {
        env::Mainnet::get_config()
    }
}

#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[error("Invalid Bitcoin address provided, expected address on network {expected:?}  but address provided is on {actual:?}")]
pub struct BitcoinAddressNetworkMismatch {
    #[serde(with = "crate::bitcoin::network")]
    expected: bitcoin::Network,
    #[serde(with = "crate::bitcoin::network")]
    actual: bitcoin::Network,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Arguments {
    pub testnet: bool,
    pub json: bool,
    pub trace: bool,
    pub config_path: PathBuf,
    pub env_config: env::Config,
    pub cmd: Command,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Start {
        resume_only: bool,
    },
    History {
        only_unfinished: bool,
    },
    Config,
    Logs {
        logs_dir: Option<PathBuf>,
        swap_id: Option<Uuid>,
        redact: bool,
    },
    WithdrawBtc {
        amount: Option<Amount>,
        address: Address,
    },
    Balance,
    Redeem {
        swap_id: Uuid,
        do_not_await_finality: bool,
    },
    Cancel {
        swap_id: Uuid,
    },
    Refund {
        swap_id: Uuid,
    },
    Punish {
        swap_id: Uuid,
    },
    SafelyAbort {
        swap_id: Uuid,
    },
    ExportBitcoinWallet,
    ExportMoneroWallet,
}

#[derive(structopt::StructOpt, Debug)]
#[structopt(
    name = "asb",
    about = "Automated Swap Backend for swapping XMR for BTC",
    author,
    version = env!("VERGEN_GIT_DESCRIBE")
)]
pub struct RawArguments {
    #[structopt(long, help = "Swap on testnet")]
    pub testnet: bool,

    #[structopt(
        short,
        long = "json",
        help = "Changes the log messages to json vs plain-text. If you run ASB as a service, it is recommended to set this to true to simplify log analyses."
    )]
    pub json: bool,

    #[structopt(long = "trace", help = "Also output verbose tracing logs to stdout")]
    pub trace: bool,

    #[structopt(
        short,
        long = "disable-timestamp",
        help = "Disable timestamping of log messages"
    )]
    pub disable_timestamp: bool,

    #[structopt(
        long = "config",
        help = "Provide a custom path to the configuration file. The configuration file must be a toml file.",
        parse(from_os_str)
    )]
    pub config: Option<PathBuf>,

    #[structopt(subcommand)]
    pub cmd: RawCommand,
}

#[derive(structopt::StructOpt, Debug)]
#[structopt(name = "xmr_btc-swap", about = "XMR BTC atomic swap")]
pub enum RawCommand {
    #[structopt(about = "Main command to run the ASB.")]
    Start {
        #[structopt(
            long = "resume-only",
            help = "For maintenance only. When set, no new swap requests will be accepted, but existing unfinished swaps will be resumed."
        )]
        resume_only: bool,
    },
    #[structopt(about = "Prints all logging messages issued in the past.")]
    Logs {
        #[structopt(
            short = "d",
            help = "Print the logs from this directory instead of the default one."
        )]
        logs_dir: Option<PathBuf>,
        #[structopt(
            help = "Redact swap-ids, Bitcoin and Monero addresses.",
            long = "redact"
        )]
        redact: bool,
        #[structopt(
            long = "swap-id",
            help = "Filter for logs concerning this swap.",
            long_help = "This checks whether each logging message contains the swap id. Some messages might be skipped when they don't contain the swap id even though they're relevant."
        )]
        swap_id: Option<Uuid>,
    },
    #[structopt(about = "Prints swap-id and the state of each swap ever made.")]
    History {
        #[structopt(long = "only-unfinished", help = "Only print in progress swaps")]
        only_unfinished: bool,
    },
    #[structopt(about = "Prints the current config")]
    Config,
    #[structopt(about = "Allows withdrawing BTC from the internal Bitcoin wallet.")]
    WithdrawBtc {
        #[structopt(
            long = "amount",
            help = "Optionally specify the amount of Bitcoin to be withdrawn. If not specified the wallet will be drained. Amount must be specified in quotes with denomination, e.g `--amount '0.1 BTC'`"
        )]
        amount: Option<Amount>,
        #[structopt(long = "address", help = "The address to receive the Bitcoin.")]
        address: Address<NetworkUnchecked>,
    },
    #[structopt(
        about = "Prints the Bitcoin and Monero balance. Requires the monero-wallet-rpc to be running."
    )]
    Balance,
    #[structopt(about = "Print the internal bitcoin wallet descriptor.")]
    ExportBitcoinWallet,
    #[structopt(about = "Print the Monero wallet seed and creation height.")]
    ExportMoneroWallet,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    const BINARY_NAME: &str = "asb";
    const BITCOIN_MAINNET_ADDRESS: &str = "1KFHE7w8BhaENAswwryaoccDb6qcT6DbYY";
    const BITCOIN_TESTNET_ADDRESS: &str = "tb1qyccwk4yun26708qg5h6g6we8kxln232wclxf5a";
    const SWAP_ID: &str = "ea030832-3be9-454f-bb98-5ea9a788406b";

    #[test]
    fn ensure_start_command_mapping_mainnet() {
        let default_mainnet_conf_path = env::Mainnet::getConfigFileDefaults().unwrap().config_path;
        let mainnet_env_config = env::Mainnet::get_config();

        let raw_ars = vec![BINARY_NAME, "start"];
        let expected_args = Arguments {
            testnet: false,
            json: false,
            trace: false,
            config_path: default_mainnet_conf_path,
            env_config: mainnet_env_config,
            cmd: Command::Start { resume_only: false },
        };
        let args = parse_args(raw_ars).unwrap();
        assert_eq!(expected_args, args);
    }

    #[test]
    fn ensure_history_command_mapping_mainnet() {
        let default_mainnet_conf_path = env::Mainnet::getConfigFileDefaults().unwrap().config_path;
        let mainnet_env_config = env::Mainnet::get_config();

        let raw_ars = vec![BINARY_NAME, "history"];
        let expected_args = Arguments {
            testnet: false,
            json: false,
            trace: false,
            config_path: default_mainnet_conf_path,
            env_config: mainnet_env_config,
            cmd: Command::History {
                only_unfinished: false,
            },
        };
        let args = parse_args(raw_ars).unwrap();
        assert_eq!(expected_args, args);
    }

    #[test]
    fn ensure_balance_command_mapping_mainnet() {
        let default_mainnet_conf_path = env::Mainnet::getConfigFileDefaults().unwrap().config_path;
        let mainnet_env_config = env::Mainnet::get_config();

        let raw_ars = vec![BINARY_NAME, "balance"];
        let expected_args = Arguments {
            testnet: false,
            json: false,
            trace: false,
            config_path: default_mainnet_conf_path,
            env_config: mainnet_env_config,
            cmd: Command::Balance,
        };
        let args = parse_args(raw_ars).unwrap();
        assert_eq!(expected_args, args);
    }

    #[test]
    fn ensure_withdraw_command_mapping_mainnet() {
        let default_mainnet_conf_path = env::Mainnet::getConfigFileDefaults().unwrap().config_path;
        let mainnet_env_config = env::Mainnet::get_config();
        let raw_ars = vec![
            BINARY_NAME,
            "withdraw-btc",
            "--address",
            BITCOIN_MAINNET_ADDRESS,
        ];
        let expected_args = Arguments {
            testnet: false,
            json: false,
            trace: false,
            config_path: default_mainnet_conf_path,
            env_config: mainnet_env_config,
            cmd: Command::WithdrawBtc {
                amount: None,
                address: bitcoin_address::parse_and_validate(BITCOIN_MAINNET_ADDRESS, false)
                    .unwrap(),
            },
        };
        let args = parse_args(raw_ars).unwrap();
        assert_eq!(expected_args, args);
    }

    #[test]
    fn ensure_cancel_command_mapping_mainnet() {
        let default_mainnet_conf_path = env::Mainnet::getConfigFileDefaults().unwrap().config_path;
        let mainnet_env_config = env::Mainnet::get_config();

        let raw_ars = vec![
            BINARY_NAME,
            "manual-recovery",
            "cancel",
            "--swap-id",
            SWAP_ID,
        ];
        let expected_args = Arguments {
            testnet: false,
            json: false,
            trace: false,
            config_path: default_mainnet_conf_path,
            env_config: mainnet_env_config,
            cmd: Command::Cancel {
                swap_id: Uuid::parse_str(SWAP_ID).unwrap(),
            },
        };
        let args = parse_args(raw_ars).unwrap();
        assert_eq!(expected_args, args);
    }

    #[test]
    fn ensure_refund_command_mappin_mainnet() {
        let default_mainnet_conf_path = env::Mainnet::getConfigFileDefaults().unwrap().config_path;
        let mainnet_env_config = env::Mainnet::get_config();

        let raw_ars = vec![
            BINARY_NAME,
            "manual-recovery",
            "refund",
            "--swap-id",
            SWAP_ID,
        ];
        let expected_args = Arguments {
            testnet: false,
            json: false,
            trace: false,
            config_path: default_mainnet_conf_path,
            env_config: mainnet_env_config,
            cmd: Command::Refund {
                swap_id: Uuid::parse_str(SWAP_ID).unwrap(),
            },
        };
        let args = parse_args(raw_ars).unwrap();
        assert_eq!(expected_args, args);
    }

    #[test]
    fn ensure_punish_command_mapping_mainnet() {
        let default_mainnet_conf_path = env::Mainnet::getConfigFileDefaults().unwrap().config_path;
        let mainnet_env_config = env::Mainnet::get_config();

        let raw_ars = vec![
            BINARY_NAME,
            "manual-recovery",
            "punish",
            "--swap-id",
            SWAP_ID,
        ];
        let expected_args = Arguments {
            testnet: false,
            json: false,
            trace: false,
            config_path: default_mainnet_conf_path,
            env_config: mainnet_env_config,
            cmd: Command::Punish {
                swap_id: Uuid::parse_str(SWAP_ID).unwrap(),
            },
        };
        let args = parse_args(raw_ars).unwrap();
        assert_eq!(expected_args, args);
    }

    #[test]
    fn ensure_safely_abort_command_mapping_mainnet() {
        let default_mainnet_conf_path = env::Mainnet::getConfigFileDefaults().unwrap().config_path;
        let mainnet_env_config = env::Mainnet::get_config();

        let raw_ars = vec![
            BINARY_NAME,
            "manual-recovery",
            "safely-abort",
            "--swap-id",
            SWAP_ID,
        ];
        let expected_args = Arguments {
            testnet: false,
            json: false,
            trace: false,
            config_path: default_mainnet_conf_path,
            env_config: mainnet_env_config,
            cmd: Command::SafelyAbort {
                swap_id: Uuid::parse_str(SWAP_ID).unwrap(),
            },
        };
        let args = parse_args(raw_ars).unwrap();
        assert_eq!(expected_args, args);
    }

    #[test]
    fn ensure_start_command_mapping_for_testnet() {
        let default_testnet_conf_path = env::Testnet::getConfigFileDefaults().unwrap().config_path;
        let testnet_env_config = env::Testnet::get_config();

        let raw_ars = vec![BINARY_NAME, "--testnet", "start"];
        let expected_args = Arguments {
            testnet: true,
            json: false,
            trace: false,
            config_path: default_testnet_conf_path,
            env_config: testnet_env_config,
            cmd: Command::Start { resume_only: false },
        };
        let args = parse_args(raw_ars).unwrap();
        assert_eq!(expected_args, args);
    }

    #[test]
    fn ensure_history_command_mapping_testnet() {
        let default_testnet_conf_path = env::Testnet::getConfigFileDefaults().unwrap().config_path;
        let testnet_env_config = env::Testnet::get_config();

        let raw_ars = vec![BINARY_NAME, "--testnet", "history"];
        let expected_args = Arguments {
            testnet: true,
            json: false,
            trace: false,
            config_path: default_testnet_conf_path,
            env_config: testnet_env_config,
            cmd: Command::History {
                only_unfinished: false,
            },
        };
        let args = parse_args(raw_ars).unwrap();
        assert_eq!(expected_args, args);
    }

    #[test]
    fn ensure_balance_command_mapping_testnet() {
        let default_testnet_conf_path = env::Testnet::getConfigFileDefaults().unwrap().config_path;
        let testnet_env_config = env::Testnet::get_config();

        let raw_ars = vec![BINARY_NAME, "--testnet", "balance"];
        let expected_args = Arguments {
            testnet: true,
            json: false,
            trace: false,
            config_path: default_testnet_conf_path,
            env_config: testnet_env_config,
            cmd: Command::Balance,
        };
        let args = parse_args(raw_ars).unwrap();
        assert_eq!(expected_args, args);
    }

    #[test]
    fn ensure_export_monero_command_mapping_testnet() {
        let default_testnet_conf_path = env::Testnet::getConfigFileDefaults().unwrap().config_path;
        let testnet_env_config = env::Testnet::get_config();

        let raw_ars = vec![BINARY_NAME, "--testnet", "export-monero-wallet"];
        let expected_args = Arguments {
            testnet: true,
            json: false,
            trace: false,
            config_path: default_testnet_conf_path,
            env_config: testnet_env_config,
            cmd: Command::ExportMoneroWallet,
        };
        let args = parse_args(raw_ars).unwrap();

        assert_eq!(expected_args, args);
    }

    #[test]
    fn ensure_withdraw_command_mapping_testnet() {
        let default_testnet_conf_path = env::Testnet::getConfigFileDefaults().unwrap().config_path;
        let testnet_env_config = env::Testnet::get_config();

        let raw_ars = vec![
            BINARY_NAME,
            "--testnet",
            "withdraw-btc",
            "--address",
            BITCOIN_TESTNET_ADDRESS,
        ];
        let expected_args = Arguments {
            testnet: true,
            json: false,
            trace: false,
            config_path: default_testnet_conf_path,
            env_config: testnet_env_config,
            cmd: Command::WithdrawBtc {
                amount: None,
                address: bitcoin_address::parse_and_validate(BITCOIN_TESTNET_ADDRESS, true)
                    .unwrap(),
            },
        };
        let args = parse_args(raw_ars).unwrap();
        assert_eq!(expected_args, args);
    }
    #[test]
    fn ensure_cancel_command_mapping_testnet() {
        let default_testnet_conf_path = env::Testnet::getConfigFileDefaults().unwrap().config_path;
        let testnet_env_config = env::Testnet::get_config();

        let raw_ars = vec![
            BINARY_NAME,
            "--testnet",
            "manual-recovery",
            "cancel",
            "--swap-id",
            SWAP_ID,
        ];
        let expected_args = Arguments {
            testnet: true,
            json: false,
            trace: false,
            config_path: default_testnet_conf_path,
            env_config: testnet_env_config,
            cmd: Command::Cancel {
                swap_id: Uuid::parse_str(SWAP_ID).unwrap(),
            },
        };
        let args = parse_args(raw_ars).unwrap();
        assert_eq!(expected_args, args);
    }

    #[test]
    fn ensure_refund_command_mapping_testnet() {
        let default_testnet_conf_path = env::Testnet::getConfigFileDefaults().unwrap().config_path;
        let testnet_env_config = env::Testnet::get_config();

        let raw_ars = vec![
            BINARY_NAME,
            "--testnet",
            "manual-recovery",
            "refund",
            "--swap-id",
            SWAP_ID,
        ];
        let expected_args = Arguments {
            testnet: true,
            json: false,
            trace: false,
            config_path: default_testnet_conf_path,
            env_config: testnet_env_config,
            cmd: Command::Refund {
                swap_id: Uuid::parse_str(SWAP_ID).unwrap(),
            },
        };
        let args = parse_args(raw_ars).unwrap();
        assert_eq!(expected_args, args);
    }

    #[test]
    fn ensure_punish_command_mapping_testnet() {
        let default_testnet_conf_path = env::Testnet::getConfigFileDefaults().unwrap().config_path;
        let testnet_env_config = env::Testnet::get_config();

        let raw_ars = vec![
            BINARY_NAME,
            "--testnet",
            "manual-recovery",
            "punish",
            "--swap-id",
            SWAP_ID,
        ];
        let expected_args = Arguments {
            testnet: true,
            json: false,
            trace: false,
            config_path: default_testnet_conf_path,
            env_config: testnet_env_config,
            cmd: Command::Punish {
                swap_id: Uuid::parse_str(SWAP_ID).unwrap(),
            },
        };
        let args = parse_args(raw_ars).unwrap();
        assert_eq!(expected_args, args);
    }

    #[test]
    fn ensure_safely_abort_command_mapping_testnet() {
        let default_testnet_conf_path = env::Testnet::getConfigFileDefaults().unwrap().config_path;
        let testnet_env_config = env::Testnet::get_config();

        let raw_ars = vec![
            BINARY_NAME,
            "--testnet",
            "manual-recovery",
            "safely-abort",
            "--swap-id",
            SWAP_ID,
        ];
        let expected_args = Arguments {
            testnet: true,
            json: false,
            trace: false,
            config_path: default_testnet_conf_path,
            env_config: testnet_env_config,
            cmd: Command::SafelyAbort {
                swap_id: Uuid::parse_str(SWAP_ID).unwrap(),
            },
        };
        let args = parse_args(raw_ars).unwrap();
        assert_eq!(expected_args, args);
    }

    #[test]
    fn ensure_disable_timestamp_mapping() {
        let default_mainnet_conf_path = env::Mainnet::getConfigFileDefaults().unwrap().config_path;
        let mainnet_env_config = env::Mainnet::get_config();

        let raw_ars = vec![BINARY_NAME, "--disable-timestamp", "start"];
        let expected_args = Arguments {
            testnet: false,
            json: false,
            trace: false,
            config_path: default_mainnet_conf_path,
            env_config: mainnet_env_config,
            cmd: Command::Start { resume_only: false },
        };
        let args = parse_args(raw_ars).unwrap();
        assert_eq!(expected_args, args);
    }

    #[test]
    fn ensure_trace_mapping() {
        let default_mainnet_conf_path = env::Mainnet::getConfigFileDefaults().unwrap().config_path;
        let mainnet_env_config = env::Mainnet::get_config();

        let raw_ars = vec![BINARY_NAME, "--trace", "start"];
        let expected_args = Arguments {
            testnet: false,
            json: false,
            trace: true,
            config_path: default_mainnet_conf_path,
            env_config: mainnet_env_config,
            cmd: Command::Start { resume_only: false },
        };
        let args = parse_args(raw_ars).unwrap();
        assert_eq!(expected_args, args);
    }

    #[test]
    fn given_user_provides_config_path_then_no_default_config_path_returned() {
        let cp = PathBuf::from_str("/some/config/path").unwrap();

        let expected = config_path(Some(cp.clone()), true).unwrap();
        assert_eq!(expected, cp);

        let expected = config_path(Some(cp.clone()), false).unwrap();
        assert_eq!(expected, cp)
    }

    #[test]
    fn given_bitcoin_address_network_mismatch_then_error() {
        let error =
            bitcoin_address::parse_and_validate(BITCOIN_TESTNET_ADDRESS, false).unwrap_err();

        let error_message = error.to_string();
        assert_eq!(
            error_message,
            "Bitcoin address network mismatch, expected `Bitcoin`"
        );

        let error = bitcoin_address::parse_and_validate(BITCOIN_MAINNET_ADDRESS, true).unwrap_err();

        let error_message = error.to_string();
        assert_eq!(
            error_message,
            "Bitcoin address network mismatch, expected `Testnet`"
        );
    }
}
