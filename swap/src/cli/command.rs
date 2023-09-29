use crate::bitcoin::Amount;
use crate::env::GetConfig;
use crate::fs::system_data_dir;
use crate::network::rendezvous::XmrBtcNamespace;
use crate::{env, monero};
use anyhow::{bail, Context, Result};
use bitcoin::{Address, AddressType};
use libp2p::core::Multiaddr;
use serde::Serialize;
use std::ffi::OsString;
use std::path::PathBuf;
use std::str::FromStr;
use structopt::{clap, StructOpt};
use url::Url;
use uuid::Uuid;

// See: https://1209k.com/bitcoin-eye/ele.php?chain=btc
const DEFAULT_ELECTRUM_RPC_URL: &str = "ssl://blockstream.info:700";
// See: https://1209k.com/bitcoin-eye/ele.php?chain=tbtc
pub const DEFAULT_ELECTRUM_RPC_URL_TESTNET: &str = "ssl://electrum.blockstream.info:60002";

const DEFAULT_BITCOIN_CONFIRMATION_TARGET: usize = 3;
const DEFAULT_BITCOIN_CONFIRMATION_TARGET_TESTNET: usize = 1;

const DEFAULT_TOR_SOCKS5_PORT: &str = "9050";

#[derive(Debug, PartialEq, Eq)]
pub struct Arguments {
    pub env_config: env::Config,
    pub debug: bool,
    pub json: bool,
    pub data_dir: PathBuf,
    pub cmd: Command,
}

/// Represents the result of parsing the command-line parameters.
#[derive(Debug, PartialEq, Eq)]
pub enum ParseResult {
    /// The arguments we were invoked in.
    Arguments(Box<Arguments>),
    /// A flag or command was given that does not need further processing other
    /// than printing the provided message.
    ///
    /// The caller should exit the program with exit code 0.
    PrintAndExitZero { message: String },
}

pub fn parse_args_and_apply_defaults<I, T>(raw_args: I) -> Result<ParseResult>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let args = match RawArguments::clap().get_matches_from_safe(raw_args) {
        Ok(matches) => RawArguments::from_clap(&matches),
        Err(clap::Error {
            message,
            kind: clap::ErrorKind::HelpDisplayed | clap::ErrorKind::VersionDisplayed,
            ..
        }) => return Ok(ParseResult::PrintAndExitZero { message }),
        Err(e) => anyhow::bail!(e),
    };

    let debug = args.debug;
    let json = args.json;
    let is_testnet = args.testnet;
    let data = args.data;

    let arguments = match args.cmd {
        RawCommand::BuyXmr {
            seller: Seller { seller },
            bitcoin,
            bitcoin_change_address,
            monero,
            monero_receive_address,
            tor: Tor { tor_socks5_port },
        } => {
            let (bitcoin_electrum_rpc_url, bitcoin_target_block) =
                bitcoin.apply_defaults(is_testnet)?;
            let monero_receive_address =
                validate_monero_address(monero_receive_address, is_testnet)?;
            let bitcoin_change_address =
                validate_bitcoin_address(bitcoin_change_address, is_testnet)?;
            let monero_daemon_address = monero.monero_daemon_address;

            Arguments {
                env_config: env_config_from(is_testnet),
                debug,
                json,
                data_dir: data::data_dir_from(data, is_testnet)?,
                cmd: Command::BuyXmr {
                    seller,
                    bitcoin_electrum_rpc_url,
                    bitcoin_target_block,
                    bitcoin_change_address,
                    monero_receive_address,
                    monero_daemon_address,
                    tor_socks5_port,
                    namespace: XmrBtcNamespace::from_is_testnet(is_testnet),
                },
            }
        }
        RawCommand::History => Arguments {
            env_config: env_config_from(is_testnet),
            debug,
            json,
            data_dir: data::data_dir_from(data, is_testnet)?,
            cmd: Command::History,
        },
        RawCommand::Config => Arguments {
            env_config: env_config_from(is_testnet),
            debug,
            json,
            data_dir: data::data_dir_from(data, is_testnet)?,
            cmd: Command::Config,
        },
        RawCommand::Balance {
            bitcoin_electrum_rpc_url,
        } => {
            let bitcoin = Bitcoin {
                bitcoin_electrum_rpc_url,
                bitcoin_target_block: None,
            };
            let (bitcoin_electrum_rpc_url, bitcoin_target_block) =
                bitcoin.apply_defaults(is_testnet)?;

            Arguments {
                env_config: env_config_from(is_testnet),
                debug,
                json,
                data_dir: data::data_dir_from(data, is_testnet)?,
                cmd: Command::Balance {
                    bitcoin_electrum_rpc_url,
                    bitcoin_target_block,
                },
            }
        }
        RawCommand::WithdrawBtc {
            bitcoin,
            amount,
            address,
        } => {
            let (bitcoin_electrum_rpc_url, bitcoin_target_block) =
                bitcoin.apply_defaults(is_testnet)?;

            Arguments {
                env_config: env_config_from(is_testnet),
                debug,
                json,
                data_dir: data::data_dir_from(data, is_testnet)?,
                cmd: Command::WithdrawBtc {
                    bitcoin_electrum_rpc_url,
                    bitcoin_target_block,
                    amount,
                    address: bitcoin_address(address, is_testnet)?,
                },
            }
        }
        RawCommand::Resume {
            swap_id: SwapId { swap_id },
            bitcoin,
            monero,
            tor: Tor { tor_socks5_port },
        } => {
            let (bitcoin_electrum_rpc_url, bitcoin_target_block) =
                bitcoin.apply_defaults(is_testnet)?;
            let monero_daemon_address = monero.monero_daemon_address;

            Arguments {
                env_config: env_config_from(is_testnet),
                debug,
                json,
                data_dir: data::data_dir_from(data, is_testnet)?,
                cmd: Command::Resume {
                    swap_id,
                    bitcoin_electrum_rpc_url,
                    bitcoin_target_block,
                    monero_daemon_address,
                    tor_socks5_port,
                    namespace: XmrBtcNamespace::from_is_testnet(is_testnet),
                },
            }
        }
        RawCommand::CancelAndRefund {
            swap_id: SwapId { swap_id },
            bitcoin,
        } => {
            let (bitcoin_electrum_rpc_url, bitcoin_target_block) =
                bitcoin.apply_defaults(is_testnet)?;

            Arguments {
                env_config: env_config_from(is_testnet),
                debug,
                json,
                data_dir: data::data_dir_from(data, is_testnet)?,
                cmd: Command::CancelAndRefund {
                    swap_id,
                    bitcoin_electrum_rpc_url,
                    bitcoin_target_block,
                },
            }
        }
        RawCommand::ListSellers {
            rendezvous_point,
            tor: Tor { tor_socks5_port },
        } => Arguments {
            env_config: env_config_from(is_testnet),
            debug,
            json,
            data_dir: data::data_dir_from(data, is_testnet)?,
            cmd: Command::ListSellers {
                rendezvous_point,
                tor_socks5_port,
                namespace: XmrBtcNamespace::from_is_testnet(is_testnet),
            },
        },
        RawCommand::ExportBitcoinWallet { bitcoin } => {
            let (bitcoin_electrum_rpc_url, bitcoin_target_block) =
                bitcoin.apply_defaults(is_testnet)?;

            Arguments {
                env_config: env_config_from(is_testnet),
                debug,
                json,
                data_dir: data::data_dir_from(data, is_testnet)?,
                cmd: Command::ExportBitcoinWallet {
                    bitcoin_electrum_rpc_url,
                    bitcoin_target_block,
                },
            }
        }
        RawCommand::MoneroRecovery { swap_id } => Arguments {
            env_config: env_config_from(is_testnet),
            debug,
            json,
            data_dir: data::data_dir_from(data, is_testnet)?,
            cmd: Command::MoneroRecovery {
                swap_id: swap_id.swap_id,
            },
        },
    };

    Ok(ParseResult::Arguments(Box::new(arguments)))
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    BuyXmr {
        seller: Multiaddr,
        bitcoin_electrum_rpc_url: Url,
        bitcoin_target_block: usize,
        bitcoin_change_address: bitcoin::Address,
        monero_receive_address: monero::Address,
        monero_daemon_address: Option<String>,
        tor_socks5_port: u16,
        namespace: XmrBtcNamespace,
    },
    History,
    Config,
    WithdrawBtc {
        bitcoin_electrum_rpc_url: Url,
        bitcoin_target_block: usize,
        amount: Option<Amount>,
        address: Address,
    },
    Balance {
        bitcoin_electrum_rpc_url: Url,
        bitcoin_target_block: usize,
    },
    Resume {
        swap_id: Uuid,
        bitcoin_electrum_rpc_url: Url,
        bitcoin_target_block: usize,
        monero_daemon_address: Option<String>,
        tor_socks5_port: u16,
        namespace: XmrBtcNamespace,
    },
    CancelAndRefund {
        swap_id: Uuid,
        bitcoin_electrum_rpc_url: Url,
        bitcoin_target_block: usize,
    },
    ListSellers {
        rendezvous_point: Multiaddr,
        namespace: XmrBtcNamespace,
        tor_socks5_port: u16,
    },
    ExportBitcoinWallet {
        bitcoin_electrum_rpc_url: Url,
        bitcoin_target_block: usize,
    },
    MoneroRecovery {
        swap_id: Uuid,
    },
}

#[derive(structopt::StructOpt, Debug)]
#[structopt(
    name = "swap",
    about = "CLI for swapping BTC for XMR",
    author,
    version = env!("VERGEN_GIT_SEMVER_LIGHTWEIGHT")
)]
struct RawArguments {
    // global is necessary to ensure that clap can match against testnet in subcommands
    #[structopt(
        long,
        help = "Swap on testnet and assume testnet defaults for data-dir and the blockchain related parameters",
        global = true
    )]
    testnet: bool,

    #[structopt(
        short,
        long = "--data-base-dir",
        help = "The base data directory to be used for mainnet / testnet specific data like database, wallets etc"
    )]
    data: Option<PathBuf>,

    #[structopt(long, help = "Activate debug logging")]
    debug: bool,

    #[structopt(
        short,
        long = "json",
        help = "Outputs all logs in JSON format instead of plain text"
    )]
    json: bool,

    #[structopt(subcommand)]
    cmd: RawCommand,
}

#[derive(structopt::StructOpt, Debug)]
enum RawCommand {
    /// Start a BTC for XMR swap
    BuyXmr {
        #[structopt(flatten)]
        seller: Seller,

        #[structopt(flatten)]
        bitcoin: Bitcoin,

        #[structopt(
            long = "change-address",
            help = "The bitcoin address where any form of change or excess funds should be sent to"
        )]
        bitcoin_change_address: bitcoin::Address,

        #[structopt(flatten)]
        monero: Monero,

        #[structopt(long = "receive-address",
            help = "The monero address where you would like to receive monero",
            parse(try_from_str = parse_monero_address)
        )]
        monero_receive_address: monero::Address,

        #[structopt(flatten)]
        tor: Tor,
    },
    /// Show a list of past, ongoing and completed swaps
    History,
    #[structopt(about = "Prints the current config")]
    Config,
    #[structopt(about = "Allows withdrawing BTC from the internal Bitcoin wallet.")]
    WithdrawBtc {
        #[structopt(flatten)]
        bitcoin: Bitcoin,

        #[structopt(
            long = "amount",
            help = "Optionally specify the amount of Bitcoin to be withdrawn. If not specified the wallet will be drained."
        )]
        amount: Option<Amount>,
        #[structopt(long = "address", help = "The address to receive the Bitcoin.")]
        address: Address,
    },
    #[structopt(about = "Prints the Bitcoin balance.")]
    Balance {
        #[structopt(long = "electrum-rpc", help = "Provide the Bitcoin Electrum RPC URL")]
        bitcoin_electrum_rpc_url: Option<Url>,
    },
    /// Resume a swap
    Resume {
        #[structopt(flatten)]
        swap_id: SwapId,

        #[structopt(flatten)]
        bitcoin: Bitcoin,

        #[structopt(flatten)]
        monero: Monero,

        #[structopt(flatten)]
        tor: Tor,
    },
    /// Force the submission of the cancel and refund transactions of a swap
    #[structopt(aliases = &["cancel", "refund"])]
    CancelAndRefund {
        #[structopt(flatten)]
        swap_id: SwapId,

        #[structopt(flatten)]
        bitcoin: Bitcoin,
    },
    /// Discover and list sellers (i.e. ASB providers)
    ListSellers {
        #[structopt(
            long,
            help = "Address of the rendezvous point you want to use to discover ASBs"
        )]
        rendezvous_point: Multiaddr,

        #[structopt(flatten)]
        tor: Tor,
    },
    /// Print the internal bitcoin wallet descriptor
    ExportBitcoinWallet {
        #[structopt(flatten)]
        bitcoin: Bitcoin,
    },
    /// Prints Monero information related to the swap in case the generated
    /// wallet fails to detect the funds. This can only be used for swaps
    /// that are in a `btc is redeemed` state.
    MoneroRecovery {
        #[structopt(flatten)]
        swap_id: SwapId,
    },
}

#[derive(structopt::StructOpt, Debug)]
struct Monero {
    #[structopt(
        long = "monero-daemon-address",
        help = "Specify to connect to a monero daemon of your choice: <host>:<port>. If none is specified, we will connect to a public node."
    )]
    monero_daemon_address: Option<String>,
}

#[derive(structopt::StructOpt, Debug)]
struct Bitcoin {
    #[structopt(long = "electrum-rpc", help = "Provide the Bitcoin Electrum RPC URL")]
    bitcoin_electrum_rpc_url: Option<Url>,

    #[structopt(
        long = "bitcoin-target-block",
        help = "Estimate Bitcoin fees such that transactions are confirmed within the specified number of blocks"
    )]
    bitcoin_target_block: Option<usize>,
}

impl Bitcoin {
    fn apply_defaults(self, testnet: bool) -> Result<(Url, usize)> {
        let bitcoin_electrum_rpc_url = if let Some(url) = self.bitcoin_electrum_rpc_url {
            url
        } else if testnet {
            Url::from_str(DEFAULT_ELECTRUM_RPC_URL_TESTNET)?
        } else {
            Url::from_str(DEFAULT_ELECTRUM_RPC_URL)?
        };

        let bitcoin_target_block = if let Some(target_block) = self.bitcoin_target_block {
            target_block
        } else if testnet {
            DEFAULT_BITCOIN_CONFIRMATION_TARGET_TESTNET
        } else {
            DEFAULT_BITCOIN_CONFIRMATION_TARGET
        };

        Ok((bitcoin_electrum_rpc_url, bitcoin_target_block))
    }
}

#[derive(structopt::StructOpt, Debug)]
struct Tor {
    #[structopt(
        long = "tor-socks5-port",
        help = "Your local Tor socks5 proxy port",
        default_value = DEFAULT_TOR_SOCKS5_PORT
    )]
    tor_socks5_port: u16,
}

#[derive(structopt::StructOpt, Debug)]
struct SwapId {
    #[structopt(
        long = "swap-id",
        help = "The swap id can be retrieved using the history subcommand"
    )]
    swap_id: Uuid,
}

#[derive(structopt::StructOpt, Debug)]
struct Seller {
    #[structopt(
        long,
        help = "The seller's address. Must include a peer ID part, i.e. `/p2p/`"
    )]
    seller: Multiaddr,
}

mod data {
    use super::*;

    pub fn data_dir_from(arg_dir: Option<PathBuf>, testnet: bool) -> Result<PathBuf> {
        let base_dir = match arg_dir {
            Some(custom_base_dir) => custom_base_dir,
            None => os_default()?,
        };

        let sub_directory = if testnet { "testnet" } else { "mainnet" };

        Ok(base_dir.join(sub_directory))
    }

    fn os_default() -> Result<PathBuf> {
        Ok(system_data_dir()?.join("cli"))
    }
}

fn env_config_from(testnet: bool) -> env::Config {
    if testnet {
        env::Testnet::get_config()
    } else {
        env::Mainnet::get_config()
    }
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

fn validate_monero_address(
    address: monero::Address,
    testnet: bool,
) -> Result<monero::Address, MoneroAddressNetworkMismatch> {
    let expected_network = if testnet {
        monero::Network::Stagenet
    } else {
        monero::Network::Mainnet
    };

    if address.network != expected_network {
        return Err(MoneroAddressNetworkMismatch {
            expected: expected_network,
            actual: address.network,
        });
    }

    Ok(address)
}

fn validate_bitcoin_address(address: bitcoin::Address, testnet: bool) -> Result<bitcoin::Address> {
    let expected_network = if testnet {
        bitcoin::Network::Testnet
    } else {
        bitcoin::Network::Bitcoin
    };

    if address.network != expected_network {
        anyhow::bail!(
            "Invalid Bitcoin address provided; expected network {} but provided address is for {}",
            expected_network,
            address.network
        );
    }

    if address.address_type() != Some(AddressType::P2wpkh) {
        anyhow::bail!("Invalid Bitcoin address provided, only bech32 format is supported!")
    }

    Ok(address)
}

fn parse_monero_address(s: &str) -> Result<monero::Address> {
    monero::Address::from_str(s).with_context(|| {
        format!(
            "Failed to parse {} as a monero address, please make sure it is a valid address",
            s
        )
    })
}

#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Eq)]
#[error("Invalid monero address provided, expected address on network {expected:?} but address provided is on {actual:?}")]
pub struct MoneroAddressNetworkMismatch {
    expected: monero::Network,
    actual: monero::Network,
}

#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[error("Invalid Bitcoin address provided, expected address on network {expected:?}  but address provided is on {actual:?}")]
pub struct BitcoinAddressNetworkMismatch {
    #[serde(with = "crate::bitcoin::network")]
    expected: bitcoin::Network,
    #[serde(with = "crate::bitcoin::network")]
    actual: bitcoin::Network,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tor::DEFAULT_SOCKS5_PORT;

    const BINARY_NAME: &str = "swap";

    const TESTNET: &str = "testnet";
    const MAINNET: &str = "mainnet";

    const MONERO_STAGENET_ADDRESS: &str = "53gEuGZUhP9JMEBZoGaFNzhwEgiG7hwQdMCqFxiyiTeFPmkbt1mAoNybEUvYBKHcnrSgxnVWgZsTvRBaHBNXPa8tHiCU51a";
    const BITCOIN_TESTNET_ADDRESS: &str = "tb1qr3em6k3gfnyl8r7q0v7t4tlnyxzgxma3lressv";
    const MONERO_MAINNET_ADDRESS: &str = "44Ato7HveWidJYUAVw5QffEcEtSH1DwzSP3FPPkHxNAS4LX9CqgucphTisH978FLHE34YNEx7FcbBfQLQUU8m3NUC4VqsRa";
    const BITCOIN_MAINNET_ADDRESS: &str = "bc1qe4epnfklcaa0mun26yz5g8k24em5u9f92hy325";
    const MULTI_ADDRESS: &str =
        "/ip4/127.0.0.1/tcp/9939/p2p/12D3KooWCdMKjesXMJz1SiZ7HgotrxuqhQJbP5sgBm2BwP1cqThi";
    const SWAP_ID: &str = "ea030832-3be9-454f-bb98-5ea9a788406b";

    #[test]
    fn given_buy_xmr_on_mainnet_then_defaults_to_mainnet() {
        let raw_ars = vec![
            BINARY_NAME,
            "buy-xmr",
            "--receive-address",
            MONERO_MAINNET_ADDRESS,
            "--change-address",
            BITCOIN_MAINNET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];

        let expected_args =
            ParseResult::Arguments(Arguments::buy_xmr_mainnet_defaults().into_boxed());
        let args = parse_args_and_apply_defaults(raw_ars).unwrap();

        assert_eq!(expected_args, args);
    }

    #[test]
    fn given_buy_xmr_on_testnet_then_defaults_to_testnet() {
        let raw_ars = vec![
            BINARY_NAME,
            "--testnet",
            "buy-xmr",
            "--receive-address",
            MONERO_STAGENET_ADDRESS,
            "--change-address",
            BITCOIN_TESTNET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];

        let args = parse_args_and_apply_defaults(raw_ars).unwrap();

        assert_eq!(
            args,
            ParseResult::Arguments(Arguments::buy_xmr_testnet_defaults().into_boxed())
        );
    }

    #[test]
    fn given_buy_xmr_on_mainnet_with_testnet_address_then_fails() {
        let raw_ars = vec![
            BINARY_NAME,
            "buy-xmr",
            "--receive-address",
            MONERO_STAGENET_ADDRESS,
            "--change-address",
            BITCOIN_TESTNET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];

        let err = parse_args_and_apply_defaults(raw_ars).unwrap_err();

        assert_eq!(
            err.downcast_ref::<MoneroAddressNetworkMismatch>().unwrap(),
            &MoneroAddressNetworkMismatch {
                expected: monero::Network::Mainnet,
                actual: monero::Network::Stagenet
            }
        );
    }

    #[test]
    fn given_buy_xmr_on_testnet_with_mainnet_address_then_fails() {
        let raw_ars = vec![
            BINARY_NAME,
            "--testnet",
            "buy-xmr",
            "--receive-address",
            MONERO_MAINNET_ADDRESS,
            "--change-address",
            BITCOIN_MAINNET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];

        let err = parse_args_and_apply_defaults(raw_ars).unwrap_err();

        assert_eq!(
            err.downcast_ref::<MoneroAddressNetworkMismatch>().unwrap(),
            &MoneroAddressNetworkMismatch {
                expected: monero::Network::Stagenet,
                actual: monero::Network::Mainnet
            }
        );
    }

    #[test]
    fn given_resume_on_mainnet_then_defaults_to_mainnet() {
        let raw_ars = vec![BINARY_NAME, "resume", "--swap-id", SWAP_ID];

        let args = parse_args_and_apply_defaults(raw_ars).unwrap();

        assert_eq!(
            args,
            ParseResult::Arguments(Arguments::resume_mainnet_defaults().into_boxed())
        );
    }

    #[test]
    fn given_resume_on_testnet_then_defaults_to_testnet() {
        let raw_ars = vec![BINARY_NAME, "--testnet", "resume", "--swap-id", SWAP_ID];

        let args = parse_args_and_apply_defaults(raw_ars).unwrap();

        assert_eq!(
            args,
            ParseResult::Arguments(Arguments::resume_testnet_defaults().into_boxed())
        );
    }

    #[test]
    fn given_cancel_on_mainnet_then_defaults_to_mainnet() {
        let raw_ars = vec![BINARY_NAME, "cancel", "--swap-id", SWAP_ID];

        let args = parse_args_and_apply_defaults(raw_ars).unwrap();

        assert_eq!(
            args,
            ParseResult::Arguments(Arguments::cancel_mainnet_defaults().into_boxed())
        );
    }

    #[test]
    fn given_cancel_on_testnet_then_defaults_to_testnet() {
        let raw_ars = vec![BINARY_NAME, "--testnet", "cancel", "--swap-id", SWAP_ID];

        let args = parse_args_and_apply_defaults(raw_ars).unwrap();

        assert_eq!(
            args,
            ParseResult::Arguments(Arguments::cancel_testnet_defaults().into_boxed())
        );
    }

    #[test]
    fn given_refund_on_mainnet_then_defaults_to_mainnet() {
        let raw_ars = vec![BINARY_NAME, "refund", "--swap-id", SWAP_ID];

        let args = parse_args_and_apply_defaults(raw_ars).unwrap();

        assert_eq!(
            args,
            ParseResult::Arguments(Arguments::refund_mainnet_defaults().into_boxed())
        );
    }

    #[test]
    fn given_refund_on_testnet_then_defaults_to_testnet() {
        let raw_ars = vec![BINARY_NAME, "--testnet", "refund", "--swap-id", SWAP_ID];

        let args = parse_args_and_apply_defaults(raw_ars).unwrap();

        assert_eq!(
            args,
            ParseResult::Arguments(Arguments::refund_testnet_defaults().into_boxed())
        );
    }

    #[test]
    fn given_with_data_dir_then_data_dir_set() {
        let data_dir = "/some/path/to/dir";

        let raw_ars = vec![
            BINARY_NAME,
            "--data-base-dir",
            data_dir,
            "buy-xmr",
            "--change-address",
            BITCOIN_MAINNET_ADDRESS,
            "--receive-address",
            MONERO_MAINNET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];

        let args = parse_args_and_apply_defaults(raw_ars).unwrap();

        assert_eq!(
            args,
            ParseResult::Arguments(
                Arguments::buy_xmr_mainnet_defaults()
                    .with_data_dir(PathBuf::from_str(data_dir).unwrap().join("mainnet"))
                    .into_boxed()
            )
        );

        let raw_ars = vec![
            BINARY_NAME,
            "--testnet",
            "--data-base-dir",
            data_dir,
            "buy-xmr",
            "--change-address",
            BITCOIN_TESTNET_ADDRESS,
            "--receive-address",
            MONERO_STAGENET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];

        let args = parse_args_and_apply_defaults(raw_ars).unwrap();

        assert_eq!(
            args,
            ParseResult::Arguments(
                Arguments::buy_xmr_testnet_defaults()
                    .with_data_dir(PathBuf::from_str(data_dir).unwrap().join("testnet"))
                    .into_boxed()
            )
        );

        let raw_ars = vec![
            BINARY_NAME,
            "--data-base-dir",
            data_dir,
            "resume",
            "--swap-id",
            SWAP_ID,
        ];

        let args = parse_args_and_apply_defaults(raw_ars).unwrap();

        assert_eq!(
            args,
            ParseResult::Arguments(
                Arguments::resume_mainnet_defaults()
                    .with_data_dir(PathBuf::from_str(data_dir).unwrap().join("mainnet"))
                    .into_boxed()
            )
        );

        let raw_ars = vec![
            BINARY_NAME,
            "--testnet",
            "--data-base-dir",
            data_dir,
            "resume",
            "--swap-id",
            SWAP_ID,
        ];

        let args = parse_args_and_apply_defaults(raw_ars).unwrap();

        assert_eq!(
            args,
            ParseResult::Arguments(
                Arguments::resume_testnet_defaults()
                    .with_data_dir(PathBuf::from_str(data_dir).unwrap().join("testnet"))
                    .into_boxed()
            )
        );
    }

    #[test]
    fn given_with_debug_then_debug_set() {
        let raw_ars = vec![
            BINARY_NAME,
            "--debug",
            "buy-xmr",
            "--change-address",
            BITCOIN_MAINNET_ADDRESS,
            "--receive-address",
            MONERO_MAINNET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];

        let args = parse_args_and_apply_defaults(raw_ars).unwrap();
        assert_eq!(
            args,
            ParseResult::Arguments(
                Arguments::buy_xmr_mainnet_defaults()
                    .with_debug()
                    .into_boxed()
            )
        );

        let raw_ars = vec![
            BINARY_NAME,
            "--testnet",
            "--debug",
            "buy-xmr",
            "--change-address",
            BITCOIN_TESTNET_ADDRESS,
            "--receive-address",
            MONERO_STAGENET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];

        let args = parse_args_and_apply_defaults(raw_ars).unwrap();
        assert_eq!(
            args,
            ParseResult::Arguments(
                Arguments::buy_xmr_testnet_defaults()
                    .with_debug()
                    .into_boxed()
            )
        );

        let raw_ars = vec![BINARY_NAME, "--debug", "resume", "--swap-id", SWAP_ID];

        let args = parse_args_and_apply_defaults(raw_ars).unwrap();
        assert_eq!(
            args,
            ParseResult::Arguments(
                Arguments::resume_mainnet_defaults()
                    .with_debug()
                    .into_boxed()
            )
        );

        let raw_ars = vec![
            BINARY_NAME,
            "--testnet",
            "--debug",
            "resume",
            "--swap-id",
            SWAP_ID,
        ];

        let args = parse_args_and_apply_defaults(raw_ars).unwrap();
        assert_eq!(
            args,
            ParseResult::Arguments(
                Arguments::resume_testnet_defaults()
                    .with_debug()
                    .into_boxed()
            )
        );
    }

    #[test]
    fn given_with_json_then_json_set() {
        let raw_ars = vec![
            BINARY_NAME,
            "--json",
            "buy-xmr",
            "--change-address",
            BITCOIN_MAINNET_ADDRESS,
            "--receive-address",
            MONERO_MAINNET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];

        let args = parse_args_and_apply_defaults(raw_ars).unwrap();
        assert_eq!(
            args,
            ParseResult::Arguments(
                Arguments::buy_xmr_mainnet_defaults()
                    .with_json()
                    .into_boxed()
            )
        );

        let raw_ars = vec![
            BINARY_NAME,
            "--testnet",
            "--json",
            "buy-xmr",
            "--change-address",
            BITCOIN_TESTNET_ADDRESS,
            "--receive-address",
            MONERO_STAGENET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];

        let args = parse_args_and_apply_defaults(raw_ars).unwrap();
        assert_eq!(
            args,
            ParseResult::Arguments(
                Arguments::buy_xmr_testnet_defaults()
                    .with_json()
                    .into_boxed()
            )
        );

        let raw_ars = vec![BINARY_NAME, "--json", "resume", "--swap-id", SWAP_ID];

        let args = parse_args_and_apply_defaults(raw_ars).unwrap();
        assert_eq!(
            args,
            ParseResult::Arguments(
                Arguments::resume_mainnet_defaults()
                    .with_json()
                    .into_boxed()
            )
        );

        let raw_ars = vec![
            BINARY_NAME,
            "--testnet",
            "--json",
            "resume",
            "--swap-id",
            SWAP_ID,
        ];

        let args = parse_args_and_apply_defaults(raw_ars).unwrap();
        assert_eq!(
            args,
            ParseResult::Arguments(
                Arguments::resume_testnet_defaults()
                    .with_json()
                    .into_boxed()
            )
        );
    }

    #[test]
    fn only_bech32_addresses_mainnet_are_allowed() {
        let raw_ars = vec![
            BINARY_NAME,
            "buy-xmr",
            "--change-address",
            "1A5btpLKZjgYm8R22rJAhdbTFVXgSRA2Mp",
            "--receive-address",
            MONERO_MAINNET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];
        let result = parse_args_and_apply_defaults(raw_ars);
        assert_eq!(
            result.unwrap_err().to_string(),
            "Invalid Bitcoin address provided, only bech32 format is supported!"
        );

        let raw_ars = vec![
            BINARY_NAME,
            "buy-xmr",
            "--change-address",
            "36vn4mFhmTXn7YcNwELFPxTXhjorw2ppu2",
            "--receive-address",
            MONERO_MAINNET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];
        let result = parse_args_and_apply_defaults(raw_ars);
        assert_eq!(
            result.unwrap_err().to_string(),
            "Invalid Bitcoin address provided, only bech32 format is supported!"
        );

        let raw_ars = vec![
            BINARY_NAME,
            "buy-xmr",
            "--change-address",
            "bc1qh4zjxrqe3trzg7s6m7y67q2jzrw3ru5mx3z7j3",
            "--receive-address",
            MONERO_MAINNET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];
        let result = parse_args_and_apply_defaults(raw_ars).unwrap();
        assert!(matches!(result, ParseResult::Arguments(_)));
    }

    #[test]
    fn only_bech32_addresses_testnet_are_allowed() {
        let raw_ars = vec![
            BINARY_NAME,
            "--testnet",
            "buy-xmr",
            "--change-address",
            "n2czxyeFCQp9e8WRyGpy4oL4YfQAeKkkUH",
            "--receive-address",
            MONERO_STAGENET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];
        let result = parse_args_and_apply_defaults(raw_ars);
        assert_eq!(
            result.unwrap_err().to_string(),
            "Invalid Bitcoin address provided, only bech32 format is supported!"
        );

        let raw_ars = vec![
            BINARY_NAME,
            "--testnet",
            "buy-xmr",
            "--change-address",
            "2ND9a4xmQG89qEWG3ETRuytjKpLmGrW7Jvf",
            "--receive-address",
            MONERO_STAGENET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];
        let result = parse_args_and_apply_defaults(raw_ars);
        assert_eq!(
            result.unwrap_err().to_string(),
            "Invalid Bitcoin address provided, only bech32 format is supported!"
        );

        let raw_ars = vec![
            BINARY_NAME,
            "--testnet",
            "buy-xmr",
            "--change-address",
            "tb1q958vfh3wkdp232pktq8zzvmttyxeqnj80zkz3v",
            "--receive-address",
            MONERO_STAGENET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];
        let result = parse_args_and_apply_defaults(raw_ars).unwrap();
        assert!(matches!(result, ParseResult::Arguments(_)));
    }

    impl Arguments {
        pub fn buy_xmr_testnet_defaults() -> Self {
            Self {
                env_config: env::Testnet::get_config(),
                debug: false,
                json: false,
                data_dir: data_dir_path_cli().join(TESTNET),
                cmd: Command::BuyXmr {
                    seller: Multiaddr::from_str(MULTI_ADDRESS).unwrap(),
                    bitcoin_electrum_rpc_url: Url::from_str(DEFAULT_ELECTRUM_RPC_URL_TESTNET)
                        .unwrap(),
                    bitcoin_target_block: DEFAULT_BITCOIN_CONFIRMATION_TARGET_TESTNET,
                    bitcoin_change_address: BITCOIN_TESTNET_ADDRESS.parse().unwrap(),
                    monero_receive_address: monero::Address::from_str(MONERO_STAGENET_ADDRESS)
                        .unwrap(),
                    monero_daemon_address: None,
                    tor_socks5_port: DEFAULT_SOCKS5_PORT,
                    namespace: XmrBtcNamespace::Testnet,
                },
            }
        }

        pub fn buy_xmr_mainnet_defaults() -> Self {
            Self {
                env_config: env::Mainnet::get_config(),
                debug: false,
                json: false,
                data_dir: data_dir_path_cli().join(MAINNET),
                cmd: Command::BuyXmr {
                    seller: Multiaddr::from_str(MULTI_ADDRESS).unwrap(),
                    bitcoin_electrum_rpc_url: Url::from_str(DEFAULT_ELECTRUM_RPC_URL).unwrap(),
                    bitcoin_target_block: DEFAULT_BITCOIN_CONFIRMATION_TARGET,
                    bitcoin_change_address: BITCOIN_MAINNET_ADDRESS.parse().unwrap(),
                    monero_receive_address: monero::Address::from_str(MONERO_MAINNET_ADDRESS)
                        .unwrap(),
                    monero_daemon_address: None,
                    tor_socks5_port: DEFAULT_SOCKS5_PORT,
                    namespace: XmrBtcNamespace::Mainnet,
                },
            }
        }

        pub fn resume_testnet_defaults() -> Self {
            Self {
                env_config: env::Testnet::get_config(),
                debug: false,
                json: false,
                data_dir: data_dir_path_cli().join(TESTNET),
                cmd: Command::Resume {
                    swap_id: Uuid::from_str(SWAP_ID).unwrap(),
                    bitcoin_electrum_rpc_url: Url::from_str(DEFAULT_ELECTRUM_RPC_URL_TESTNET)
                        .unwrap(),
                    bitcoin_target_block: DEFAULT_BITCOIN_CONFIRMATION_TARGET_TESTNET,
                    monero_daemon_address: None,
                    tor_socks5_port: DEFAULT_SOCKS5_PORT,
                    namespace: XmrBtcNamespace::Testnet,
                },
            }
        }

        pub fn resume_mainnet_defaults() -> Self {
            Self {
                env_config: env::Mainnet::get_config(),
                debug: false,
                json: false,
                data_dir: data_dir_path_cli().join(MAINNET),
                cmd: Command::Resume {
                    swap_id: Uuid::from_str(SWAP_ID).unwrap(),
                    bitcoin_electrum_rpc_url: Url::from_str(DEFAULT_ELECTRUM_RPC_URL).unwrap(),
                    bitcoin_target_block: DEFAULT_BITCOIN_CONFIRMATION_TARGET,
                    monero_daemon_address: None,
                    tor_socks5_port: DEFAULT_SOCKS5_PORT,
                    namespace: XmrBtcNamespace::Mainnet,
                },
            }
        }

        pub fn cancel_testnet_defaults() -> Self {
            Self {
                env_config: env::Testnet::get_config(),
                debug: false,
                json: false,
                data_dir: data_dir_path_cli().join(TESTNET),
                cmd: Command::CancelAndRefund {
                    swap_id: Uuid::from_str(SWAP_ID).unwrap(),
                    bitcoin_electrum_rpc_url: Url::from_str(DEFAULT_ELECTRUM_RPC_URL_TESTNET)
                        .unwrap(),
                    bitcoin_target_block: DEFAULT_BITCOIN_CONFIRMATION_TARGET_TESTNET,
                },
            }
        }

        pub fn cancel_mainnet_defaults() -> Self {
            Self {
                env_config: env::Mainnet::get_config(),
                debug: false,
                json: false,
                data_dir: data_dir_path_cli().join(MAINNET),
                cmd: Command::CancelAndRefund {
                    swap_id: Uuid::from_str(SWAP_ID).unwrap(),
                    bitcoin_electrum_rpc_url: Url::from_str(DEFAULT_ELECTRUM_RPC_URL).unwrap(),
                    bitcoin_target_block: DEFAULT_BITCOIN_CONFIRMATION_TARGET,
                },
            }
        }

        pub fn refund_testnet_defaults() -> Self {
            Self {
                env_config: env::Testnet::get_config(),
                debug: false,
                json: false,
                data_dir: data_dir_path_cli().join(TESTNET),
                cmd: Command::CancelAndRefund {
                    swap_id: Uuid::from_str(SWAP_ID).unwrap(),
                    bitcoin_electrum_rpc_url: Url::from_str(DEFAULT_ELECTRUM_RPC_URL_TESTNET)
                        .unwrap(),
                    bitcoin_target_block: DEFAULT_BITCOIN_CONFIRMATION_TARGET_TESTNET,
                },
            }
        }

        pub fn refund_mainnet_defaults() -> Self {
            Self {
                env_config: env::Mainnet::get_config(),
                debug: false,
                json: false,
                data_dir: data_dir_path_cli().join(MAINNET),
                cmd: Command::CancelAndRefund {
                    swap_id: Uuid::from_str(SWAP_ID).unwrap(),
                    bitcoin_electrum_rpc_url: Url::from_str(DEFAULT_ELECTRUM_RPC_URL).unwrap(),
                    bitcoin_target_block: DEFAULT_BITCOIN_CONFIRMATION_TARGET,
                },
            }
        }

        pub fn with_data_dir(mut self, data_dir: PathBuf) -> Self {
            self.data_dir = data_dir;
            self
        }

        pub fn with_debug(mut self) -> Self {
            self.debug = true;
            self
        }

        pub fn with_json(mut self) -> Self {
            self.json = true;
            self
        }

        pub fn into_boxed(self) -> Box<Self> {
            Box::new(self)
        }
    }

    fn data_dir_path_cli() -> PathBuf {
        system_data_dir().unwrap().join("cli")
    }
}
