use crate::bitcoin::Amount;
use crate::{env, monero};
use crate::api::{Request, Params, Context};
use anyhow::{bail, Context as AnyContext, Result};
use bitcoin::{Address, AddressType};
use libp2p::core::Multiaddr;
use serde::Serialize;
use std::ffi::OsString;
use std::path::PathBuf;
use std::str::FromStr;
use structopt::{clap, StructOpt};
use url::Url;
use uuid::Uuid;
use std::net::SocketAddr;
use std::sync::Arc;
use crate::fs::system_data_dir;

// See: https://moneroworld.com/
pub const DEFAULT_MONERO_DAEMON_ADDRESS: &str = "node.community.rino.io:18081";
pub const DEFAULT_MONERO_DAEMON_ADDRESS_STAGENET: &str = "stagenet.community.rino.io:38081";

// See: https://1209k.com/bitcoin-eye/ele.php?chain=btc
const DEFAULT_ELECTRUM_RPC_URL: &str = "ssl://blockstream.info:700";
// See: https://1209k.com/bitcoin-eye/ele.php?chain=tbtc
pub const DEFAULT_ELECTRUM_RPC_URL_TESTNET: &str = "ssl://electrum.blockstream.info:60002";

const DEFAULT_BITCOIN_CONFIRMATION_TARGET: usize = 3;
pub const DEFAULT_BITCOIN_CONFIRMATION_TARGET_TESTNET: usize = 1;

const DEFAULT_TOR_SOCKS5_PORT: &str = "9050";

/// Represents the result of parsing the command-line parameters.

#[derive(Debug)]
pub enum ParseResult {
    /// The arguments we were invoked in.
    Context(Arc<Context>, Box<Request>),
    /// A flag or command was given that does not need further processing other
    /// than printing the provided message.
    ///
    /// The caller should exit the program with exit code 0.
    PrintAndExitZero { message: String },
}

pub async fn parse_args_and_apply_defaults<I, T>(raw_args: I) -> Result<ParseResult>
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

    let (context, request) = match args.cmd {
        RawCommand::BuyXmr {
            seller: Seller { seller },
            bitcoin,
            bitcoin_change_address,
            monero,
            monero_receive_address,
            tor,
        } => {
            let context = Context::build(
                Some(bitcoin),
                Some(monero),
                Some(tor),
                data,
                is_testnet,
                debug,
                json,
                None
            ).await?;
            let request = Request {
                params: Params {
                    bitcoin_change_address: Some(bitcoin_change_address),
                    monero_receive_address: Some(monero_receive_address),
                    seller: Some(seller),
                    ..Default::default()
                },
                cmd: Command::BuyXmr,
            };
            (context, request)
        }
        RawCommand::History => {
            let context = Context::build(
                None,
                None,
                None,
                data,
                is_testnet,
                debug,
                json,
                None
            ).await?;

            let request = Request {
                params: Params::default(),
                cmd: Command::History,
            };
            (context, request)
        },
        RawCommand::Config => {
            let context = Context::build(
                None,
                None,
                None,
                data,
                is_testnet,
                debug,
                json,
                None
            ).await?;

            let request = Request {
                params: Params::default(),
                cmd: Command::Config,
            };
            (context, request)
        },
        RawCommand::Balance {
            bitcoin,
        } => {
            let context = Context::build(
                Some(bitcoin),
                None,
                None,
                data,
                is_testnet,
                debug,
                json,
                None
            ).await?;
            let request = Request {
                params: Params::default(),
                cmd: Command::Balance,
            };
            (context, request)
        }
        RawCommand::StartDaemon {
            server_address,
            bitcoin,
            monero,
            tor,
        } => {
            let context = Context::build(
                Some(bitcoin),
                Some(monero),
                Some(tor),
                data,
                is_testnet,
                debug,
                json,
                server_address,
            ).await?;
            let request = Request {
                params: Params::default(),
                cmd: Command::StartDaemon,
            };
            (context, request)
        }
        RawCommand::WithdrawBtc {
            bitcoin,
            amount,
            address,
        } => {
            let context = Context::build(
                Some(bitcoin),
                None,
                None,
                data,
                is_testnet,
                debug,
                json,
                None
            ).await?;
            let request = Request {
                params: Params {
                    amount: amount,
                    address: Some(address),
                    ..Default::default()
                },
                cmd: Command::WithdrawBtc,
            };
            (context, request)
        }
        RawCommand::Resume {
            swap_id: SwapId { swap_id },
            bitcoin,
            monero,
            tor,
        } => {
            let context = Context::build(
                Some(bitcoin),
                Some(monero),
                Some(tor),
                data,
                is_testnet,
                debug,
                json,
                None,
            ).await?;
            let request = Request {
                params: Params {
                    swap_id: Some(swap_id),
                    ..Default::default()
                },
                cmd: Command::Resume,
            };
            (context, request)
        }
        RawCommand::Cancel {
            swap_id: SwapId { swap_id },
            bitcoin,
        } => {
            let context = Context::build(
                Some(bitcoin),
                None,
                None,
                data,
                is_testnet,
                debug,
                json,
                None
            ).await?;
            let request = Request {
                params: Params {
                    swap_id: Some(swap_id),
                    ..Default::default()
                },
                cmd: Command::Cancel,
            };
            (context, request)
        }
        RawCommand::Refund {
            swap_id: SwapId { swap_id },
            bitcoin,
        } => {
            let context = Context::build(
                Some(bitcoin),
                None,
                None,
                data,
                is_testnet,
                debug,
                json,
                None
            ).await?;
            let request = Request {
                params: Params {
                    swap_id: Some(swap_id),
                    ..Default::default()
                },
                cmd: Command::Refund,
            };
            (context, request)
        }
        RawCommand::ListSellers {
            rendezvous_point,
            tor,
        } => {
            let context = Context::build(
                None,
                None,
                Some(tor),
                data,
                is_testnet,
                debug,
                json,
                None
            ).await?;

            let request = Request {
                params: Params {
                    rendezvous_point: Some(rendezvous_point),
                    ..Default::default()
                },
                cmd: Command::ListSellers,
            };
            (context, request)
        }
        RawCommand::ExportBitcoinWallet { bitcoin } => {
            let context = Context::build(
                Some(bitcoin),
                None,
                None,
                data,
                is_testnet,
                debug,
                json,
                None
            ).await?;
            let request = Request {
                params: Params::default(),
                cmd: Command::ExportBitcoinWallet,
            };
            (context, request)
        },
        RawCommand::MoneroRecovery { swap_id } => {
            let context = Context::build(
                None,
                None,
                None,
                data,
                is_testnet,
                debug,
                json,
                None
            ).await?;

            let request = Request {
                params: Params {
                    swap_id: Some(swap_id.swap_id),
                    ..Default::default()
                },
                cmd: Command::MoneroRecovery,
            };
            (context, request)
        },
    };

    Ok(ParseResult::Context(Arc::new(context), Box::new(request)))
}
#[derive(Debug, PartialEq)]
pub enum Command {
    BuyXmr,
    History,
    Config,
    WithdrawBtc,
    Balance,
    Resume,
    Cancel,
    Refund,
    ListSellers,
    ExportBitcoinWallet,
    MoneroRecovery,
    StartDaemon,
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
        #[structopt(flatten)]
        bitcoin: Bitcoin,
    },
    #[structopt(about="Starts a JSON-RPC server")]
    StartDaemon {
        #[structopt(flatten)]
        bitcoin: Bitcoin,

        #[structopt(flatten)]
        monero: Monero,
        
        #[structopt(long="server-address", help = "The socket address the server should use")]
        server_address: Option<SocketAddr>,
        #[structopt(flatten)]
        tor: Tor,
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
    /// Force submission of the cancel transaction overriding the protocol state
    /// machine and blockheight checks (expert users only)
    Cancel {
        #[structopt(flatten)]
        swap_id: SwapId,

        #[structopt(flatten)]
        bitcoin: Bitcoin,
    },
    /// Force submission of the refund transaction overriding the protocol state
    /// machine and blockheight checks (expert users only)
    Refund {
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
pub struct Monero {
    #[structopt(
        long = "monero-daemon-address",
        help = "Specify to connect to a monero daemon of your choice: <host>:<port>"
    )]
    pub monero_daemon_address: Option<String>,
}

impl Monero {
    pub fn apply_defaults(self, testnet: bool) -> String {
        if let Some(address) = self.monero_daemon_address {
            address
        } else if testnet {
            DEFAULT_MONERO_DAEMON_ADDRESS_STAGENET.to_string()
        } else {
            DEFAULT_MONERO_DAEMON_ADDRESS.to_string()
        }
    }
}

#[derive(structopt::StructOpt, Debug)]
pub struct Bitcoin {
    #[structopt(long = "electrum-rpc", help = "Provide the Bitcoin Electrum RPC URL")]
    pub bitcoin_electrum_rpc_url: Option<Url>,

    #[structopt(
        long = "bitcoin-target-block",
        help = "Estimate Bitcoin fees such that transactions are confirmed within the specified number of blocks"
    )]
    pub bitcoin_target_block: Option<usize>,
}

impl Bitcoin {
    pub fn apply_defaults(self, testnet: bool) -> Result<(Url, usize)> {
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
pub struct Tor {
    #[structopt(
        long = "tor-socks5-port",
        help = "Your local Tor socks5 proxy port",
        default_value = DEFAULT_TOR_SOCKS5_PORT
    )]
    pub tor_socks5_port: u16,
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

#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq)]
#[error("Invalid monero address provided, expected address on network {expected:?} but address provided is on {actual:?}")]
pub struct MoneroAddressNetworkMismatch {
    expected: monero::Network,
    actual: monero::Network,
}

#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Serialize)]
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

    use crate::api::api_test::*;

    const BINARY_NAME: &str = "swap";

    #[tokio::test]
    async fn given_buy_xmr_on_mainnet_then_defaults_to_mainnet() {
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

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (false, false, false);
        let data_dir = data_dir_path_cli(is_testnet);

        let (expected_context, expected_request) =
            (Context::default(is_testnet, data_dir, debug, json).await.unwrap(), Request::buy_xmr(is_testnet));

        let (actual_context, actual_request) = match args {
            ParseResult::Context(context, request) => (context, request),
            _ => panic!("Couldn't parse result")
        };

        assert_eq!(actual_context, Arc::new(expected_context));
        assert_eq!(actual_request, Box::new(expected_request));
    }

    #[tokio::test]
    async fn given_buy_xmr_on_testnet_then_defaults_to_testnet() {
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

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (true, false, false);
        let data_dir = data_dir_path_cli(is_testnet);

        let (expected_context, expected_request) =
            (Context::default(is_testnet, data_dir, debug, json).await.unwrap(), Request::buy_xmr(is_testnet));

        let (actual_context, actual_request) = match args {
            ParseResult::Context(context, request) => (context, request),
            _ => panic!("Couldn't parse result")
        };

        assert_eq!(actual_context, Arc::new(expected_context));
        assert_eq!(actual_request, Box::new(expected_request));
    }

    #[tokio::test]
    async fn given_buy_xmr_on_mainnet_with_testnet_address_then_fails() {
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

        let err = parse_args_and_apply_defaults(raw_ars).await.unwrap_err();

        assert_eq!(
            err.downcast_ref::<MoneroAddressNetworkMismatch>().unwrap(),
            &MoneroAddressNetworkMismatch {
                expected: monero::Network::Mainnet,
                actual: monero::Network::Stagenet
            }
        );
    }

    #[tokio::test]
    async fn given_buy_xmr_on_testnet_with_mainnet_address_then_fails() {
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

        let err = parse_args_and_apply_defaults(raw_ars).await.unwrap_err();

        assert_eq!(
            err.downcast_ref::<MoneroAddressNetworkMismatch>().unwrap(),
            &MoneroAddressNetworkMismatch {
                expected: monero::Network::Stagenet,
                actual: monero::Network::Mainnet
            }
        );
    }

    #[tokio::test]
    async fn given_resume_on_mainnet_then_defaults_to_mainnet() {
        let raw_ars = vec![BINARY_NAME, "resume", "--swap-id", SWAP_ID];

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (false, false, false);
        let data_dir = data_dir_path_cli(is_testnet);

        let (expected_context, expected_request) =
            (Context::default(is_testnet, data_dir, debug, json).await.unwrap(), Request::resume());

        let (actual_context, actual_request) = match args {
            ParseResult::Context(context, request) => (context, request),
            _ => panic!("Couldn't parse result")
        };

        assert_eq!(actual_context, Arc::new(expected_context));
        assert_eq!(actual_request, Box::new(expected_request));
    }

    #[tokio::test]
    async fn given_resume_on_testnet_then_defaults_to_testnet() {
        let raw_ars = vec![BINARY_NAME, "--testnet", "resume", "--swap-id", SWAP_ID];

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (true, false, false);
        let data_dir = data_dir_path_cli(is_testnet);

        let (expected_context, expected_request) =
            (Context::default(is_testnet, data_dir, debug, json).await.unwrap(), Request::resume());

        let (actual_context, actual_request) = match args {
            ParseResult::Context(context, request) => (context, request),
            _ => panic!("Couldn't parse result")
        };

        assert_eq!(actual_context, Arc::new(expected_context));
        assert_eq!(actual_request, Box::new(expected_request));
    }

    #[tokio::test]
    async fn given_cancel_on_mainnet_then_defaults_to_mainnet() {
        let raw_ars = vec![BINARY_NAME, "cancel", "--swap-id", SWAP_ID];

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();

        let (is_testnet, debug, json) = (false, false, false);
        let data_dir = data_dir_path_cli(is_testnet);

        let (expected_context, expected_request) =
            (Context::default(is_testnet, data_dir, debug, json).await.unwrap(), Request::cancel());

        let (actual_context, actual_request) = match args {
            ParseResult::Context(context, request) => (context, request),
            _ => panic!("Couldn't parse result")
        };

        assert_eq!(actual_context, Arc::new(expected_context));
        assert_eq!(actual_request, Box::new(expected_request));
    }

    #[tokio::test]
    async fn given_cancel_on_testnet_then_defaults_to_testnet() {
        let raw_ars = vec![BINARY_NAME, "--testnet", "cancel", "--swap-id", SWAP_ID];

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (true, false, false);
        let data_dir = data_dir_path_cli(is_testnet);

        let (expected_context, expected_request) =
            (Context::default(is_testnet, data_dir, debug, json).await.unwrap(), Request::cancel());

        let (actual_context, actual_request) = match args {
            ParseResult::Context(context, request) => (context, request),
            _ => panic!("Couldn't parse result")
        };

        assert_eq!(actual_context, Arc::new(expected_context));
        assert_eq!(actual_request, Box::new(expected_request));
    }

    #[tokio::test]
    async fn given_refund_on_mainnet_then_defaults_to_mainnet() {
        let raw_ars = vec![BINARY_NAME, "refund", "--swap-id", SWAP_ID];

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (false, false, false);
        let data_dir = data_dir_path_cli(is_testnet);

        let (expected_context, expected_request) =
            (Context::default(is_testnet, data_dir, debug, json).await.unwrap(), Request::refund());

        let (actual_context, actual_request) = match args {
            ParseResult::Context(context, request) => (context, request),
            _ => panic!("Couldn't parse result")
        };

        assert_eq!(actual_context, Arc::new(expected_context));
        assert_eq!(actual_request, Box::new(expected_request));
    }

    #[tokio::test]
    async fn given_refund_on_testnet_then_defaults_to_testnet() {
        let raw_ars = vec![BINARY_NAME, "--testnet", "refund", "--swap-id", SWAP_ID];

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (true, false, false);
        let data_dir = data_dir_path_cli(is_testnet);

        let (expected_context, expected_request) =
            (Context::default(is_testnet, data_dir, debug, json).await.unwrap(), Request::refund());

        let (actual_context, actual_request) = match args {
            ParseResult::Context(context, request) => (context, request),
            _ => panic!("Couldn't parse result")
        };

        assert_eq!(actual_context, Arc::new(expected_context));
        assert_eq!(actual_request, Box::new(expected_request));
    }

    #[tokio::test]
    async fn given_with_data_dir_then_data_dir_set() {
        let args_data_dir = "/some/path/to/dir";

        let raw_ars = vec![
            BINARY_NAME,
            "--data-base-dir",
            args_data_dir,
            "buy-xmr",
            "--change-address",
            BITCOIN_MAINNET_ADDRESS,
            "--receive-address",
            MONERO_MAINNET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (false, false, false);
        let data_dir = PathBuf::from_str(args_data_dir).unwrap();

        let (expected_context, expected_request) =
            (Context::default(is_testnet, data_dir.clone(), debug, json).await.unwrap(), Request::buy_xmr(is_testnet));

        let (actual_context, actual_request) = match args {
            ParseResult::Context(context, request) => (context, request),
            _ => panic!("Couldn't parse result")
        };

        assert_eq!(actual_context, Arc::new(expected_context));
        assert_eq!(actual_request, Box::new(expected_request));

        let raw_ars = vec![
            BINARY_NAME,
            "--testnet",
            "--data-base-dir",
            args_data_dir,
            "buy-xmr",
            "--change-address",
            BITCOIN_TESTNET_ADDRESS,
            "--receive-address",
            MONERO_STAGENET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (true, false, false);

        let (expected_context, expected_request) =
            (Context::default(is_testnet, data_dir.clone(), debug, json).await.unwrap(), Request::buy_xmr(is_testnet));

        let (actual_context, actual_request) = match args {
            ParseResult::Context(context, request) => (context, request),
            _ => panic!("Couldn't parse result")
        };

        assert_eq!(actual_context, Arc::new(expected_context));
        assert_eq!(actual_request, Box::new(expected_request));

        let raw_ars = vec![
            BINARY_NAME,
            "--data-base-dir",
            args_data_dir,
            "resume",
            "--swap-id",
            SWAP_ID,
        ];

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (false, false, false);

        let (expected_context, expected_request) =
            (Context::default(is_testnet, data_dir.clone(), debug, json).await.unwrap(), Request::resume());

        let (actual_context, actual_request) = match args {
            ParseResult::Context(context, request) => (context, request),
            _ => panic!("Couldn't parse result")
        };

        assert_eq!(actual_context, Arc::new(expected_context));
        assert_eq!(actual_request, Box::new(expected_request));

        let raw_ars = vec![
            BINARY_NAME,
            "--testnet",
            "--data-base-dir",
            args_data_dir,
            "resume",
            "--swap-id",
            SWAP_ID,
        ];
        


        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (true, false, false);

        let (expected_context, expected_request) =
            (Context::default(is_testnet, data_dir.clone(), debug, json).await.unwrap(), Request::resume());

        let (actual_context, actual_request) = match args {
            ParseResult::Context(context, request) => (context, request),
            _ => panic!("Couldn't parse result")
        };

        assert_eq!(actual_context, Arc::new(expected_context));
        assert_eq!(actual_request, Box::new(expected_request));
    }

    #[tokio::test]
    async fn given_with_debug_then_debug_set() {
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

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (false, true, false);
        let data_dir = data_dir_path_cli(is_testnet);

        let (expected_context, expected_request) =
            (Context::default(is_testnet, data_dir, debug, json).await.unwrap(), Request::buy_xmr(is_testnet));

        let (actual_context, actual_request) = match args {
            ParseResult::Context(context, request) => (context, request),
            _ => panic!("Couldn't parse result")
        };

        assert_eq!(actual_context, Arc::new(expected_context));
        assert_eq!(actual_request, Box::new(expected_request));

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

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (true, true, false);
        let data_dir = data_dir_path_cli(is_testnet);

        let (expected_context, expected_request) =
            (Context::default(is_testnet, data_dir, debug, json).await.unwrap(), Request::buy_xmr(is_testnet));

        let (actual_context, actual_request) = match args {
            ParseResult::Context(context, request) => (context, request),
            _ => panic!("Couldn't parse result")
        };

        assert_eq!(actual_context, Arc::new(expected_context));
        assert_eq!(actual_request, Box::new(expected_request));

        let raw_ars = vec![BINARY_NAME, "--debug", "resume", "--swap-id", SWAP_ID];

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (false, true, false);
        let data_dir = data_dir_path_cli(is_testnet);

        let (expected_context, expected_request) =
            (Context::default(is_testnet, data_dir, debug, json).await.unwrap(), Request::resume());

        let (actual_context, actual_request) = match args {
            ParseResult::Context(context, request) => (context, request),
            _ => panic!("Couldn't parse result")
        };

        assert_eq!(actual_context, Arc::new(expected_context));
        assert_eq!(actual_request, Box::new(expected_request));

        let raw_ars = vec![
            BINARY_NAME,
            "--testnet",
            "--debug",
            "resume",
            "--swap-id",
            SWAP_ID,
        ];

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (true, true, false);
        let data_dir = data_dir_path_cli(is_testnet);

        let (expected_context, expected_request) =
            (Context::default(is_testnet, data_dir, debug, json).await.unwrap(), Request::resume());

        let (actual_context, actual_request) = match args {
            ParseResult::Context(context, request) => (context, request),
            _ => panic!("Couldn't parse result")
        };

        assert_eq!(actual_context, Arc::new(expected_context));
        assert_eq!(actual_request, Box::new(expected_request));
    }

    #[tokio::test]
    async fn given_with_json_then_json_set() {
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

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (false, false, true);
        let data_dir = data_dir_path_cli(is_testnet);

        let (expected_context, expected_request) =
            (Context::default(is_testnet, data_dir, debug, json).await.unwrap(), Request::buy_xmr(is_testnet));

        let (actual_context, actual_request) = match args {
            ParseResult::Context(context, request) => (context, request),
            _ => panic!("Couldn't parse result")
        };

        assert_eq!(actual_context, Arc::new(expected_context));
        assert_eq!(actual_request, Box::new(expected_request));

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

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (true, false, true);
        let data_dir = data_dir_path_cli(is_testnet);

        let (expected_context, expected_request) =
            (Context::default(is_testnet, data_dir, debug, json).await.unwrap(), Request::buy_xmr(is_testnet));

        let (actual_context, actual_request) = match args {
            ParseResult::Context(context, request) => (context, request),
            _ => panic!("Couldn't parse result")
        };

        assert_eq!(actual_context, Arc::new(expected_context));
        assert_eq!(actual_request, Box::new(expected_request));

        let raw_ars = vec![BINARY_NAME, "--json", "resume", "--swap-id", SWAP_ID];
        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (false, false, true);
        let data_dir = data_dir_path_cli(is_testnet);

        let (expected_context, expected_request) =
            (Context::default(is_testnet, data_dir, debug, json).await.unwrap(), Request::resume());

        let (actual_context, actual_request) = match args {
            ParseResult::Context(context, request) => (context, request),
            _ => panic!("Couldn't parse result")
        };

        assert_eq!(actual_context, Arc::new(expected_context));
        assert_eq!(actual_request, Box::new(expected_request));

        let raw_ars = vec![
            BINARY_NAME,
            "--testnet",
            "--json",
            "resume",
            "--swap-id",
            SWAP_ID,
        ];

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (true, false, true);
        let data_dir = data_dir_path_cli(is_testnet);

        let (expected_context, expected_request) =
            (Context::default(is_testnet, data_dir, debug, json).await.unwrap(), Request::resume());

        let (actual_context, actual_request) = match args {
            ParseResult::Context(context, request) => (context, request),
            _ => panic!("Couldn't parse result")
        };

        assert_eq!(actual_context, Arc::new(expected_context));
        assert_eq!(actual_request, Box::new(expected_request));
    }

    #[tokio::test]
    async fn only_bech32_addresses_mainnet_are_allowed() {
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
        let args = parse_args_and_apply_defaults(raw_ars).await;
        let (is_testnet, debug, json) = (false, false, false);
        let data_dir = data_dir_path_cli(is_testnet);

        assert_eq!(
            args.unwrap_err().to_string(),
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
            result.await.unwrap_err().to_string(),
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
        let result = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        //assert!(matches!(result, ParseResult::Arguments(_)));
        assert!(true);
    }

    #[tokio::test]
    async fn only_bech32_addresses_testnet_are_allowed() {
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
            result.await.unwrap_err().to_string(),
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
            result.await.unwrap_err().to_string(),
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
        let result = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        //assert!(matches!(result, ParseResult::Arguments(_)));
        assert!(true);
    }

    fn data_dir_path_cli(is_testnet: bool) -> PathBuf {
        if is_testnet {
            system_data_dir().unwrap().join("cli").join("testnet")
        } else {
            system_data_dir().unwrap().join("cli").join("mainnet")
        }
    }
}
