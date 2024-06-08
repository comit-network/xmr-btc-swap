use crate::api::request::{Method, Request};
use crate::api::Context;
use crate::bitcoin::{bitcoin_address, Amount};
use crate::monero;
use crate::monero::monero_address;
use anyhow::Result;
use libp2p::core::Multiaddr;
use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use structopt::{clap, StructOpt};
use url::Url;
use uuid::Uuid;

// See: https://moneroworld.com/
pub const DEFAULT_MONERO_DAEMON_ADDRESS: &str = "node.community.rino.io:18081";
pub const DEFAULT_MONERO_DAEMON_ADDRESS_STAGENET: &str = "stagenet.community.rino.io:38081";

// See: https://1209k.com/bitcoin-eye/ele.php?chain=btc
const DEFAULT_ELECTRUM_RPC_URL: &str = "ssl://blockstream.info:700";
// See: https://1209k.com/bitcoin-eye/ele.php?chain=tbtc
pub const DEFAULT_ELECTRUM_RPC_URL_TESTNET: &str = "ssl://electrum.blockstream.info:60002";

const DEFAULT_BITCOIN_CONFIRMATION_TARGET: usize = 1;
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
    let args = match Arguments::clap().get_matches_from_safe(raw_args) {
        Ok(matches) => Arguments::from_clap(&matches),
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
        CliCommand::AttemptCooperativeRelease { swap_id: SwapId { swap_id } } => {
            let request = Request::new(Method::AttemptCooperativeRelease {
                swap_id: swap_id,
            });

            let context =
                Context::build(None, None, None, data, is_testnet, debug, json, None).await?;
            (context, request)
        }
        CliCommand::BuyXmr {
            seller: Seller { seller },
            bitcoin,
            bitcoin_change_address,
            monero,
            monero_receive_address,
            tor,
        } => {
            let monero_receive_address =
                monero_address::validate_is_testnet(monero_receive_address, is_testnet)?;
            let bitcoin_change_address =
                bitcoin_address::validate_is_testnet(bitcoin_change_address, is_testnet)?;

            let request = Request::new(Method::BuyXmr {
                seller,
                bitcoin_change_address,
                monero_receive_address,
                swap_id: Uuid::new_v4(),
            });

            let context = Context::build(
                Some(bitcoin),
                Some(monero),
                Some(tor),
                data,
                is_testnet,
                debug,
                json,
                None,
            )
            .await?;
            (context, request)
        }
        CliCommand::History => {
            let request = Request::new(Method::History);

            let context =
                Context::build(None, None, None, data, is_testnet, debug, json, None).await?;
            (context, request)
        }
        CliCommand::Config => {
            let request = Request::new(Method::Config);

            let context =
                Context::build(None, None, None, data, is_testnet, debug, json, None).await?;
            (context, request)
        }
        CliCommand::Balance { bitcoin } => {
            let request = Request::new(Method::Balance {
                force_refresh: true,
            });

            let context = Context::build(
                Some(bitcoin),
                None,
                None,
                data,
                is_testnet,
                debug,
                json,
                None,
            )
            .await?;
            (context, request)
        }
        CliCommand::StartDaemon {
            server_address,
            bitcoin,
            monero,
            tor,
        } => {
            let request = Request::new(Method::StartDaemon { server_address });

            let context = Context::build(
                Some(bitcoin),
                Some(monero),
                Some(tor),
                data,
                is_testnet,
                debug,
                json,
                server_address,
            )
            .await?;
            (context, request)
        }
        CliCommand::WithdrawBtc {
            bitcoin,
            amount,
            address,
        } => {
            let address = bitcoin_address::validate_is_testnet(address, is_testnet)?;
            let request = Request::new(Method::WithdrawBtc { amount, address });

            let context = Context::build(
                Some(bitcoin),
                None,
                None,
                data,
                is_testnet,
                debug,
                json,
                None,
            )
            .await?;
            (context, request)
        }
        CliCommand::Resume {
            swap_id: SwapId { swap_id },
            bitcoin,
            monero,
            tor,
        } => {
            let request = Request::new(Method::Resume { swap_id });

            let context = Context::build(
                Some(bitcoin),
                Some(monero),
                Some(tor),
                data,
                is_testnet,
                debug,
                json,
                None,
            )
            .await?;
            (context, request)
        }
        CliCommand::CancelAndRefund {
            swap_id: SwapId { swap_id },
            bitcoin,
            tor,
        } => {
            let request = Request::new(Method::CancelAndRefund { swap_id });

            let context = Context::build(
                Some(bitcoin),
                None,
                Some(tor),
                data,
                is_testnet,
                debug,
                json,
                None,
            )
            .await?;
            (context, request)
        }
        CliCommand::ListSellers {
            rendezvous_point,
            tor,
        } => {
            let request = Request::new(Method::ListSellers { rendezvous_point });

            let context =
                Context::build(None, None, Some(tor), data, is_testnet, debug, json, None).await?;

            (context, request)
        }
        CliCommand::ExportBitcoinWallet { bitcoin } => {
            let request = Request::new(Method::ExportBitcoinWallet);

            let context = Context::build(
                Some(bitcoin),
                None,
                None,
                data,
                is_testnet,
                debug,
                json,
                None,
            )
            .await?;
            (context, request)
        }
        CliCommand::MoneroRecovery {
            swap_id: SwapId { swap_id },
        } => {
            let request = Request::new(Method::MoneroRecovery { swap_id });

            let context =
                Context::build(None, None, None, data, is_testnet, debug, json, None).await?;

            (context, request)
        }
    };

    Ok(ParseResult::Context(Arc::new(context), Box::new(request)))
}

#[derive(structopt::StructOpt, Debug)]
#[structopt(
    name = "swap",
    about = "CLI for swapping BTC for XMR",
    author,
    version = env!("VERGEN_GIT_DESCRIBE")
)]
struct Arguments {
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
    cmd: CliCommand,
}

#[derive(structopt::StructOpt, Debug)]
enum CliCommand {
    #[structopt(about = "Asks Alice for XMR key to redeem funds, if Bob is punished by Alice.")]
    AttemptCooperativeRelease {
        #[structopt(flatten)]
        swap_id: SwapId,
    },
    /// Start a BTC for XMR swap
    BuyXmr {
        #[structopt(flatten)]
        seller: Seller,

        #[structopt(flatten)]
        bitcoin: Bitcoin,

        #[structopt(
            long = "change-address",
            help = "The bitcoin address where any form of change or excess funds should be sent to",
            parse(try_from_str = bitcoin_address::parse)
        )]
        bitcoin_change_address: bitcoin::Address,

        #[structopt(flatten)]
        monero: Monero,

        #[structopt(long = "receive-address",
            help = "The monero address where you would like to receive monero",
            parse(try_from_str = monero_address::parse)
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

        #[structopt(long = "address",
            help = "The address to receive the Bitcoin.",
            parse(try_from_str = bitcoin_address::parse)
        )]
        address: bitcoin::Address,
    },
    #[structopt(about = "Prints the Bitcoin balance.")]
    Balance {
        #[structopt(flatten)]
        bitcoin: Bitcoin,
    },
    #[structopt(about = "Starts a JSON-RPC server")]
    StartDaemon {
        #[structopt(flatten)]
        bitcoin: Bitcoin,

        #[structopt(flatten)]
        monero: Monero,

        #[structopt(
            long = "server-address",
            help = "The socket address the server should use"
        )]
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
    /// Force the submission of the cancel and refund transactions of a swap
    #[structopt(aliases = &["cancel", "refund"])]
    CancelAndRefund {
        #[structopt(flatten)]
        swap_id: SwapId,

        #[structopt(flatten)]
        bitcoin: Bitcoin,

        #[structopt(flatten)]
        tor: Tor,
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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::api::api_test::*;
    use crate::api::Config;
    use crate::monero::monero_address::MoneroAddressNetworkMismatch;

    const BINARY_NAME: &str = "swap";
    const ARGS_DATA_DIR: &str = "/tmp/dir/";

    #[tokio::test]

    // this test is very long, however it just checks that various CLI arguments sets the
    // internal Context and Request properly. It is unlikely to fail and splitting it in various
    // tests would require to run the tests sequantially which is very slow (due to the context
    // need to access files like the Bitcoin wallet).
    async fn test_cli_arguments() {
        // given_buy_xmr_on_mainnet_then_defaults_to_mainnet

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

        let (actual_config, actual_request) = match args {
            ParseResult::Context(context, request) => (context.config.clone(), request),
            _ => panic!("Couldn't parse result"),
        };

        let (expected_config, mut expected_request) = (
            Config::default(is_testnet, None, debug, json),
            Request::buy_xmr(is_testnet),
        );

        // since Uuid is random, copy before comparing requests
        if let Method::BuyXmr {
            ref mut swap_id, ..
        } = expected_request.cmd
        {
            *swap_id = match actual_request.cmd {
                Method::BuyXmr { swap_id, .. } => swap_id,
                _ => panic!("Not the Method we expected"),
            }
        };

        assert_eq!(actual_config, expected_config);
        assert_eq!(actual_request, Box::new(expected_request));

        // given_buy_xmr_on_testnet_then_defaults_to_testnet
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

        let (expected_config, mut expected_request) = (
            Config::default(is_testnet, None, debug, json),
            Request::buy_xmr(is_testnet),
        );

        let (actual_config, actual_request) = match args {
            ParseResult::Context(context, request) => (context.config.clone(), request),
            _ => panic!("Couldn't parse result"),
        };

        if let Method::BuyXmr {
            ref mut swap_id, ..
        } = expected_request.cmd
        {
            *swap_id = match actual_request.cmd {
                Method::BuyXmr { swap_id, .. } => swap_id,
                _ => panic!("Not the Method we expected"),
            }
        };

        assert_eq!(actual_config, expected_config);
        assert_eq!(actual_request, Box::new(expected_request));

        // given_buy_xmr_on_mainnet_with_testnet_address_then_fails
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

        // given_buy_xmr_on_testnet_with_mainnet_address_then_fails
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

        // given_resume_on_mainnet_then_defaults_to_mainnet
        let raw_ars = vec![BINARY_NAME, "resume", "--swap-id", SWAP_ID];

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (false, false, false);

        let (expected_config, expected_request) = (
            Config::default(is_testnet, None, debug, json),
            Request::resume(),
        );

        let (actual_config, actual_request) = match args {
            ParseResult::Context(context, request) => (context.config.clone(), request),
            _ => panic!("Couldn't parse result"),
        };

        assert_eq!(actual_config, expected_config);
        assert_eq!(actual_request, Box::new(expected_request));

        // given_resume_on_testnet_then_defaults_to_testnet
        let raw_ars = vec![BINARY_NAME, "--testnet", "resume", "--swap-id", SWAP_ID];

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (true, false, false);

        let (expected_config, expected_request) = (
            Config::default(is_testnet, None, debug, json),
            Request::resume(),
        );

        let (actual_config, actual_request) = match args {
            ParseResult::Context(context, request) => (context.config.clone(), request),
            _ => panic!("Couldn't parse result"),
        };

        assert_eq!(actual_config, expected_config);
        assert_eq!(actual_request, Box::new(expected_request));

        // given_cancel_on_mainnet_then_defaults_to_mainnet
        let raw_ars = vec![BINARY_NAME, "cancel", "--swap-id", SWAP_ID];

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();

        let (is_testnet, debug, json) = (false, false, false);

        let (expected_config, expected_request) = (
            Config::default(is_testnet, None, debug, json),
            Request::cancel(),
        );

        let (actual_config, actual_request) = match args {
            ParseResult::Context(context, request) => (context.config.clone(), request),
            _ => panic!("Couldn't parse result"),
        };

        assert_eq!(actual_config, expected_config);
        assert_eq!(actual_request, Box::new(expected_request));

        // given_cancel_on_testnet_then_defaults_to_testnet
        let raw_ars = vec![BINARY_NAME, "--testnet", "cancel", "--swap-id", SWAP_ID];

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (true, false, false);

        let (expected_config, expected_request) = (
            Config::default(is_testnet, None, debug, json),
            Request::cancel(),
        );

        let (actual_config, actual_request) = match args {
            ParseResult::Context(context, request) => (context.config.clone(), request),
            _ => panic!("Couldn't parse result"),
        };

        assert_eq!(actual_config, expected_config);
        assert_eq!(actual_request, Box::new(expected_request));

        let raw_ars = vec![BINARY_NAME, "refund", "--swap-id", SWAP_ID];

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (false, false, false);

        let (expected_config, expected_request) = (
            Config::default(is_testnet, None, debug, json),
            Request::refund(),
        );

        let (actual_config, actual_request) = match args {
            ParseResult::Context(context, request) => (context.config.clone(), request),
            _ => panic!("Couldn't parse result"),
        };

        assert_eq!(actual_config, expected_config);
        assert_eq!(actual_request, Box::new(expected_request));

        // given_refund_on_testnet_then_defaults_to_testnet
        let raw_ars = vec![BINARY_NAME, "--testnet", "refund", "--swap-id", SWAP_ID];

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (true, false, false);

        let (expected_config, expected_request) = (
            Config::default(is_testnet, None, debug, json),
            Request::refund(),
        );

        let (actual_config, actual_request) = match args {
            ParseResult::Context(context, request) => (context.config.clone(), request),
            _ => panic!("Couldn't parse result"),
        };

        assert_eq!(actual_config, expected_config);
        assert_eq!(actual_request, Box::new(expected_request));

        // given_buy_xmr_on_mainnet_with_data_dir_then_data_dir_set
        let raw_ars = vec![
            BINARY_NAME,
            "--data-base-dir",
            ARGS_DATA_DIR,
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
        let data_dir = PathBuf::from_str(ARGS_DATA_DIR).unwrap();

        let (expected_config, mut expected_request) = (
            Config::default(is_testnet, Some(data_dir.clone()), debug, json),
            Request::buy_xmr(is_testnet),
        );

        let (actual_config, actual_request) = match args {
            ParseResult::Context(context, request) => (context.config.clone(), request),
            _ => panic!("Couldn't parse result"),
        };

        if let Method::BuyXmr {
            ref mut swap_id, ..
        } = expected_request.cmd
        {
            *swap_id = match actual_request.cmd {
                Method::BuyXmr { swap_id, .. } => swap_id,
                _ => panic!("Not the Method we expected"),
            }
        };

        assert_eq!(actual_config, expected_config);
        assert_eq!(actual_request, Box::new(expected_request));

        // given_buy_xmr_on_testnet_with_data_dir_then_data_dir_set
        let raw_ars = vec![
            BINARY_NAME,
            "--testnet",
            "--data-base-dir",
            ARGS_DATA_DIR,
            "buy-xmr",
            "--change-address",
            BITCOIN_TESTNET_ADDRESS,
            "--receive-address",
            MONERO_STAGENET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];

        let data_dir = PathBuf::from_str(ARGS_DATA_DIR).unwrap();
        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (true, false, false);

        let (expected_config, mut expected_request) = (
            Config::default(is_testnet, Some(data_dir.clone()), debug, json),
            Request::buy_xmr(is_testnet),
        );

        let (actual_config, actual_request) = match args {
            ParseResult::Context(context, request) => (context.config.clone(), request),
            _ => panic!("Couldn't parse result"),
        };

        if let Method::BuyXmr {
            ref mut swap_id, ..
        } = expected_request.cmd
        {
            *swap_id = match actual_request.cmd {
                Method::BuyXmr { swap_id, .. } => swap_id,
                _ => panic!("Not the Method we expected"),
            }
        };

        assert_eq!(actual_config, expected_config);
        assert_eq!(actual_request, Box::new(expected_request));

        // given_resume_on_mainnet_with_data_dir_then_data_dir_set
        let raw_ars = vec![
            BINARY_NAME,
            "--data-base-dir",
            ARGS_DATA_DIR,
            "resume",
            "--swap-id",
            SWAP_ID,
        ];

        let data_dir = PathBuf::from_str(ARGS_DATA_DIR).unwrap();
        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (false, false, false);

        let (expected_config, mut expected_request) = (
            Config::default(is_testnet, Some(data_dir.clone()), debug, json),
            Request::resume(),
        );

        if let Method::BuyXmr {
            ref mut swap_id, ..
        } = expected_request.cmd
        {
            *swap_id = match actual_request.cmd {
                Method::BuyXmr { swap_id, .. } => swap_id,
                _ => panic!("Not the Method we expected"),
            }
        };

        let (actual_config, actual_request) = match args {
            ParseResult::Context(context, request) => (context.config.clone(), request),
            _ => panic!("Couldn't parse result"),
        };

        assert_eq!(actual_config, expected_config);
        assert_eq!(actual_request, Box::new(expected_request));

        // given_resume_on_testnet_with_data_dir_then_data_dir_set
        let raw_ars = vec![
            BINARY_NAME,
            "--testnet",
            "--data-base-dir",
            ARGS_DATA_DIR,
            "resume",
            "--swap-id",
            SWAP_ID,
        ];

        let data_dir = PathBuf::from_str(ARGS_DATA_DIR).unwrap();
        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (true, false, false);

        let (expected_config, expected_request) = (
            Config::default(is_testnet, Some(data_dir.clone()), debug, json),
            Request::resume(),
        );

        let (actual_config, actual_request) = match args {
            ParseResult::Context(context, request) => (context.config.clone(), request),
            _ => panic!("Couldn't parse result"),
        };

        assert_eq!(actual_config, expected_config);
        assert_eq!(actual_request, Box::new(expected_request));

        // given_buy_xmr_on_mainnet_with_debug_then_debug_set
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

        let (expected_config, mut expected_request) = (
            Config::default(is_testnet, None, debug, json),
            Request::buy_xmr(is_testnet),
        );

        let (actual_config, actual_request) = match args {
            ParseResult::Context(context, request) => (context.config.clone(), request),
            _ => panic!("Couldn't parse result"),
        };

        if let Method::BuyXmr {
            ref mut swap_id, ..
        } = expected_request.cmd
        {
            *swap_id = match actual_request.cmd {
                Method::BuyXmr { swap_id, .. } => swap_id,
                _ => panic!("Not the Method we expected"),
            }
        };

        assert_eq!(actual_config, expected_config);
        assert_eq!(actual_request, Box::new(expected_request));

        // given_buy_xmr_on_testnet_with_debug_then_debug_set
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

        let (expected_config, mut expected_request) = (
            Config::default(is_testnet, None, debug, json),
            Request::buy_xmr(is_testnet),
        );

        let (actual_config, actual_request) = match args {
            ParseResult::Context(context, request) => (context.config.clone(), request),
            _ => panic!("Couldn't parse result"),
        };

        if let Method::BuyXmr {
            ref mut swap_id, ..
        } = expected_request.cmd
        {
            *swap_id = match actual_request.cmd {
                Method::BuyXmr { swap_id, .. } => swap_id,
                _ => panic!("Not the Method we expected"),
            }
        };

        assert_eq!(actual_config, expected_config);
        assert_eq!(actual_request, Box::new(expected_request));

        // given_resume_on_mainnet_with_debug_then_debug_set
        let raw_ars = vec![BINARY_NAME, "--debug", "resume", "--swap-id", SWAP_ID];

        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (false, true, false);

        let (expected_config, mut expected_request) = (
            Config::default(is_testnet, None, debug, json),
            Request::resume(),
        );

        let (actual_config, actual_request) = match args {
            ParseResult::Context(context, request) => (context.config.clone(), request),
            _ => panic!("Couldn't parse result"),
        };

        if let Method::BuyXmr {
            ref mut swap_id, ..
        } = expected_request.cmd
        {
            *swap_id = match actual_request.cmd {
                Method::BuyXmr { swap_id, .. } => swap_id,
                _ => panic!("Not the Method we expected"),
            }
        };

        assert_eq!(actual_config, expected_config);
        assert_eq!(actual_request, Box::new(expected_request));

        // given_resume_on_testnet_with_debug_then_debug_set
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

        let (expected_config, expected_request) = (
            Config::default(is_testnet, None, debug, json),
            Request::resume(),
        );

        let (actual_config, actual_request) = match args {
            ParseResult::Context(context, request) => (context.config.clone(), request),
            _ => panic!("Couldn't parse result"),
        };

        assert_eq!(actual_config, expected_config);
        assert_eq!(actual_request, Box::new(expected_request));

        // given_buy_xmr_on_mainnet_with_json_then_json_set
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

        let (expected_config, mut expected_request) = (
            Config::default(is_testnet, None, debug, json),
            Request::buy_xmr(is_testnet),
        );

        let (actual_config, actual_request) = match args {
            ParseResult::Context(context, request) => (context.config.clone(), request),
            _ => panic!("Couldn't parse result"),
        };

        if let Method::BuyXmr {
            ref mut swap_id, ..
        } = expected_request.cmd
        {
            *swap_id = match actual_request.cmd {
                Method::BuyXmr { swap_id, .. } => swap_id,
                _ => panic!("Not the Method we expected"),
            }
        };

        assert_eq!(actual_config, expected_config);
        assert_eq!(actual_request, Box::new(expected_request));

        // given_buy_xmr_on_testnet_with_json_then_json_set
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

        let (is_testnet, debug, json) = (true, false, true);

        let (expected_config, mut expected_request) = (
            Config::default(is_testnet, None, debug, json),
            Request::buy_xmr(is_testnet),
        );
        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();

        let (actual_config, actual_request) = match args {
            ParseResult::Context(context, request) => (context.config.clone(), request),
            _ => panic!("Couldn't parse result"),
        };

        if let Method::BuyXmr {
            ref mut swap_id, ..
        } = expected_request.cmd
        {
            *swap_id = match actual_request.cmd {
                Method::BuyXmr { swap_id, .. } => swap_id,
                _ => panic!("Not the Method we expected"),
            }
        };

        assert_eq!(actual_config, expected_config);
        assert_eq!(actual_request, Box::new(expected_request));

        // given_resume_on_mainnet_with_json_then_json_set
        let raw_ars = vec![BINARY_NAME, "--json", "resume", "--swap-id", SWAP_ID];
        let args = parse_args_and_apply_defaults(raw_ars).await.unwrap();
        let (is_testnet, debug, json) = (false, false, true);

        let (expected_config, expected_request) = (
            Config::default(is_testnet, None, debug, json),
            Request::resume(),
        );

        let (actual_config, actual_request) = match args {
            ParseResult::Context(context, request) => (context.config.clone(), request),
            _ => panic!("Couldn't parse result"),
        };

        assert_eq!(actual_config, expected_config);
        assert_eq!(actual_request, Box::new(expected_request));

        // given_resume_on_testnet_with_json_then_json_set
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

        let (expected_config, expected_request) = (
            Config::default(is_testnet, None, debug, json),
            Request::resume(),
        );

        let (actual_config, actual_request) = match args {
            ParseResult::Context(context, request) => (context.config.clone(), request),
            _ => panic!("Couldn't parse result"),
        };

        assert_eq!(actual_config, expected_config);
        assert_eq!(actual_request, Box::new(expected_request));

        // only_bech32_addresses_mainnet_are_allowed
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
        parse_args_and_apply_defaults(raw_ars).await.unwrap_err();

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
        parse_args_and_apply_defaults(raw_ars).await.unwrap_err();

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
        assert!(matches!(result, ParseResult::Context(_, _)));

        // only_bech32_addresses_testnet_are_allowed
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
        parse_args_and_apply_defaults(raw_ars).await.unwrap_err();

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
        parse_args_and_apply_defaults(raw_ars).await.unwrap_err();

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
        assert!(matches!(result, ParseResult::Context(_, _)));
    }
}
