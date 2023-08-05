use crate::asb::config::GetDefaults;
use crate::bitcoin::Amount;
use crate::env;
use crate::env::GetConfig;
use anyhow::{bail, Result};
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
    let disable_timestamp = args.disable_timestamp;
    let testnet = args.testnet;
    let config = args.config;
    let command: RawCommand = args.cmd;

    let arguments = match command {
        RawCommand::Start { resume_only } => Arguments {
            testnet,
            json,
            disable_timestamp,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::Start { resume_only },
        },
        RawCommand::History => Arguments {
            testnet,
            json,
            disable_timestamp,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::History,
        },
        RawCommand::WithdrawBtc { amount, address } => Arguments {
            testnet,
            json,
            disable_timestamp,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::WithdrawBtc {
                amount,
                address: bitcoin_address(address, testnet)?,
            },
        },
        RawCommand::Balance => Arguments {
            testnet,
            json,
            disable_timestamp,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::Balance,
        },
        RawCommand::Config => Arguments {
            testnet,
            json,
            disable_timestamp,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::Config,
        },
        RawCommand::ExportBitcoinWallet => Arguments {
            testnet,
            json,
            disable_timestamp,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::ExportBitcoinWallet,
        },
        RawCommand::ManualRecovery(ManualRecovery::Redeem {
            redeem_params: RecoverCommandParams { swap_id },
            do_not_await_finality,
        }) => Arguments {
            testnet,
            json,
            disable_timestamp,
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
            disable_timestamp,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::Cancel { swap_id },
        },
        RawCommand::ManualRecovery(ManualRecovery::Refund {
            refund_params: RecoverCommandParams { swap_id },
        }) => Arguments {
            testnet,
            json,
            disable_timestamp,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::Refund { swap_id },
        },
        RawCommand::ManualRecovery(ManualRecovery::Punish {
            punish_params: RecoverCommandParams { swap_id },
        }) => Arguments {
            testnet,
            json,
            disable_timestamp,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::Punish { swap_id },
        },
        RawCommand::ManualRecovery(ManualRecovery::SafelyAbort { swap_id }) => Arguments {
            testnet,
            json,
            disable_timestamp,
            config_path: config_path(config, testnet)?,
            env_config: env_config(testnet),
            cmd: Command::SafelyAbort { swap_id },
        },
    };

    Ok(arguments)
}

fn bitcoin_address(address: Address, is_testnet: bool) -> Result<Address> {
    let network = if is_testnet {
        bitcoin::Network::Testnet
    } else {
        bitcoin::Network::Bitcoin
    };

    if address.network != network {
        bail!(BitcoinAddressNetworkMismatch {
            expected: network,
            actual: address.network
        });
    }

    Ok(address)
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
    pub disable_timestamp: bool,
    pub config_path: PathBuf,
    pub env_config: env::Config,
    pub cmd: Command,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Start {
        resume_only: bool
    },
    History,
    Config,
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
}

#[derive(structopt::StructOpt, Debug)]
#[structopt(
    name = "asb",
    about = "Automated Swap Backend for swapping XMR for BTC",
    author,
    version = env!("VERGEN_GIT_SEMVER_LIGHTWEIGHT")
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
        resume_only: bool
    },
    #[structopt(about = "Prints swap-id and the state of each swap ever made.")]
    History,
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
        address: Address,
    },
    #[structopt(
        about = "Prints the Bitcoin and Monero balance. Requires the monero-wallet-rpc to be running."
    )]
    Balance,
    #[structopt(about = "Print the internal bitcoin wallet descriptor.")]
    ExportBitcoinWallet,
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
            disable_timestamp: false,
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
            disable_timestamp: false,
            config_path: default_mainnet_conf_path,
            env_config: mainnet_env_config,
            cmd: Command::History,
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
            disable_timestamp: false,
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
            disable_timestamp: false,
            config_path: default_mainnet_conf_path,
            env_config: mainnet_env_config,
            cmd: Command::WithdrawBtc {
                amount: None,
                address: Address::from_str(BITCOIN_MAINNET_ADDRESS).unwrap(),
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
            disable_timestamp: false,
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
            disable_timestamp: false,
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
            disable_timestamp: false,
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
            disable_timestamp: false,
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
            disable_timestamp: false,
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
            disable_timestamp: false,
            config_path: default_testnet_conf_path,
            env_config: testnet_env_config,
            cmd: Command::History,
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
            disable_timestamp: false,
            config_path: default_testnet_conf_path,
            env_config: testnet_env_config,
            cmd: Command::Balance,
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
            disable_timestamp: false,
            config_path: default_testnet_conf_path,
            env_config: testnet_env_config,
            cmd: Command::WithdrawBtc {
                amount: None,
                address: Address::from_str(BITCOIN_TESTNET_ADDRESS).unwrap(),
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
            disable_timestamp: false,
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
            disable_timestamp: false,
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
            disable_timestamp: false,
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
            disable_timestamp: false,
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
            disable_timestamp: true,
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
            bitcoin_address(Address::from_str(BITCOIN_MAINNET_ADDRESS).unwrap(), true).unwrap_err();

        assert_eq!(
            error
                .downcast_ref::<BitcoinAddressNetworkMismatch>()
                .unwrap(),
            &BitcoinAddressNetworkMismatch {
                expected: bitcoin::Network::Testnet,
                actual: bitcoin::Network::Bitcoin
            }
        );

        let error = bitcoin_address(Address::from_str(BITCOIN_TESTNET_ADDRESS).unwrap(), false)
            .unwrap_err();

        assert_eq!(
            error
                .downcast_ref::<BitcoinAddressNetworkMismatch>()
                .unwrap(),
            &BitcoinAddressNetworkMismatch {
                expected: bitcoin::Network::Bitcoin,
                actual: bitcoin::Network::Testnet
            }
        );
    }
}
