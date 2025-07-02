use crate::bitcoin::{bitcoin_address, Amount};
use crate::cli::api::request::{
    BalanceArgs, BuyXmrArgs, CancelAndRefundArgs, ExportBitcoinWalletArgs, GetConfigArgs,
    GetHistoryArgs, ListSellersArgs, MoneroRecoveryArgs, Request, ResumeSwapArgs, WithdrawBtcArgs,
};
use crate::cli::api::Context;
use crate::monero::monero_address;
use crate::monero::{self, MoneroAddressPool};
use anyhow::Result;
use bitcoin::address::NetworkUnchecked;
use libp2p::core::Multiaddr;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use structopt::{clap, StructOpt};
use url::Url;
use uuid::Uuid;

use super::api::request::GetLogsArgs;
use super::api::ContextBuilder;

// See: https://1209k.com/bitcoin-eye/ele.php?chain=btc
const DEFAULT_ELECTRUM_RPC_URL: &str = "ssl://blockstream.info:700";
// See: https://1209k.com/bitcoin-eye/ele.php?chain=tbtc
pub const DEFAULT_ELECTRUM_RPC_URL_TESTNET: &str = "tcp://electrum.blockstream.info:60001";

const DEFAULT_BITCOIN_CONFIRMATION_TARGET: u16 = 1;
pub const DEFAULT_BITCOIN_CONFIRMATION_TARGET_TESTNET: u16 = 1;

/// Represents the result of parsing the command-line parameters.

#[derive(Debug)]
pub enum ParseResult {
    /// The arguments we were invoked in.
    Success(Arc<Context>),
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
    let result: Result<Arc<Context>> = match args.cmd {
        CliCommand::BuyXmr {
            seller: Seller { seller },
            bitcoin,
            bitcoin_change_address,
            monero,
            monero_receive_address,
            tor,
        } => {
            let monero_receive_pool: MoneroAddressPool =
                monero_address::validate_is_testnet(monero_receive_address, is_testnet)?.into();

            let bitcoin_change_address = bitcoin_change_address
                .map(|address| bitcoin_address::validate(address, is_testnet))
                .transpose()?
                .map(|address| address.into_unchecked());

            let context = Arc::new(
                ContextBuilder::new(is_testnet)
                    .with_tor(tor.enable_tor)
                    .with_bitcoin(bitcoin)
                    .with_monero(monero)
                    .with_data_dir(data)
                    .with_debug(debug)
                    .with_json(json)
                    .build()
                    .await?,
            );

            BuyXmrArgs {
                rendezvous_points: vec![],
                sellers: vec![seller],
                bitcoin_change_address,
                monero_receive_pool,
            }
            .request(context.clone())
            .await?;

            Ok(context)
        }
        CliCommand::History => {
            let context = Arc::new(
                ContextBuilder::new(is_testnet)
                    .with_data_dir(data)
                    .with_debug(debug)
                    .with_json(json)
                    .build()
                    .await?,
            );

            GetHistoryArgs {}.request(context.clone()).await?;

            Ok(context)
        }
        CliCommand::Logs {
            logs_dir,
            redact,
            swap_id,
        } => {
            let context = Arc::new(
                ContextBuilder::new(is_testnet)
                    .with_data_dir(data)
                    .with_debug(debug)
                    .with_json(json)
                    .build()
                    .await?,
            );

            GetLogsArgs {
                logs_dir,
                redact,
                swap_id,
            }
            .request(context.clone())
            .await?;

            Ok(context)
        }
        CliCommand::Config => {
            let context = Arc::new(
                ContextBuilder::new(is_testnet)
                    .with_data_dir(data)
                    .with_debug(debug)
                    .with_json(json)
                    .build()
                    .await?,
            );

            GetConfigArgs {}.request(context.clone()).await?;

            Ok(context)
        }
        CliCommand::Balance { bitcoin } => {
            let context = Arc::new(
                ContextBuilder::new(is_testnet)
                    .with_bitcoin(bitcoin)
                    .with_data_dir(data)
                    .with_debug(debug)
                    .with_json(json)
                    .build()
                    .await?,
            );

            BalanceArgs {
                force_refresh: true,
            }
            .request(context.clone())
            .await?;

            Ok(context)
        }
        CliCommand::WithdrawBtc {
            bitcoin,
            amount,
            address,
        } => {
            let address = bitcoin_address::validate(address, is_testnet)?;

            let context = Arc::new(
                ContextBuilder::new(is_testnet)
                    .with_bitcoin(bitcoin)
                    .with_data_dir(data)
                    .with_debug(debug)
                    .with_json(json)
                    .build()
                    .await?,
            );

            WithdrawBtcArgs { amount, address }
                .request(context.clone())
                .await?;

            Ok(context)
        }
        CliCommand::Resume {
            swap_id: SwapId { swap_id },
            bitcoin,
            monero,
            tor,
        } => {
            let context = Arc::new(
                ContextBuilder::new(is_testnet)
                    .with_tor(tor.enable_tor)
                    .with_bitcoin(bitcoin)
                    .with_monero(monero)
                    .with_data_dir(data)
                    .with_debug(debug)
                    .with_json(json)
                    .build()
                    .await?,
            );

            ResumeSwapArgs { swap_id }.request(context.clone()).await?;

            Ok(context)
        }
        CliCommand::CancelAndRefund {
            swap_id: SwapId { swap_id },
            bitcoin,
        } => {
            let context = Arc::new(
                ContextBuilder::new(is_testnet)
                    .with_bitcoin(bitcoin)
                    .with_data_dir(data)
                    .with_debug(debug)
                    .with_json(json)
                    .build()
                    .await?,
            );

            CancelAndRefundArgs { swap_id }
                .request(context.clone())
                .await?;

            Ok(context)
        }
        CliCommand::ListSellers {
            rendezvous_point,
            tor,
        } => {
            let context = Arc::new(
                ContextBuilder::new(is_testnet)
                    .with_tor(tor.enable_tor)
                    .with_data_dir(data)
                    .with_debug(debug)
                    .with_json(json)
                    .build()
                    .await?,
            );

            ListSellersArgs {
                rendezvous_points: vec![rendezvous_point],
            }
            .request(context.clone())
            .await?;

            Ok(context)
        }
        CliCommand::ExportBitcoinWallet { bitcoin } => {
            let context = Arc::new(
                ContextBuilder::new(is_testnet)
                    .with_bitcoin(bitcoin)
                    .with_data_dir(data)
                    .with_debug(debug)
                    .with_json(json)
                    .build()
                    .await?,
            );

            ExportBitcoinWalletArgs {}.request(context.clone()).await?;

            Ok(context)
        }
        CliCommand::MoneroRecovery {
            swap_id: SwapId { swap_id },
        } => {
            let context = Arc::new(
                ContextBuilder::new(is_testnet)
                    .with_data_dir(data)
                    .with_debug(debug)
                    .with_json(json)
                    .build()
                    .await?,
            );

            MoneroRecoveryArgs { swap_id }
                .request(context.clone())
                .await?;

            Ok(context)
        }
    };

    Ok(ParseResult::Success(result?))
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
    /// Start a BTC for XMR swap
    BuyXmr {
        #[structopt(flatten)]
        seller: Seller,

        #[structopt(flatten)]
        bitcoin: Bitcoin,

        #[structopt(
            long = "change-address",
            help = "The bitcoin address where any form of change or excess funds should be sent to. If omitted they will be sent to the internal wallet.",
            parse(try_from_str = bitcoin_address::parse)
        )]
        bitcoin_change_address: Option<bitcoin::Address<NetworkUnchecked>>,

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
    /// Output all logging messages that have been issued.
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
        address: bitcoin::Address<NetworkUnchecked>,
    },
    #[structopt(about = "Prints the Bitcoin balance.")]
    Balance {
        #[structopt(flatten)]
        bitcoin: Bitcoin,
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
pub struct Monero {
    #[structopt(
        long = "monero-node-address",
        help = "Specify to connect to a monero node of your choice: <host>:<port>"
    )]
    pub monero_node_address: Option<Url>,
}

#[derive(structopt::StructOpt, Debug)]
pub struct Bitcoin {
    #[structopt(long = "electrum-rpc", help = "Provide the Bitcoin Electrum RPC URLs")]
    pub bitcoin_electrum_rpc_urls: Vec<String>,

    #[structopt(
        long = "bitcoin-target-block",
        help = "Estimate Bitcoin fees such that transactions are confirmed within the specified number of blocks"
    )]
    pub bitcoin_target_block: Option<u16>,
}

impl Bitcoin {
    pub fn apply_defaults(self, testnet: bool) -> Result<(Vec<String>, u16)> {
        let bitcoin_electrum_rpc_urls = if !self.bitcoin_electrum_rpc_urls.is_empty() {
            self.bitcoin_electrum_rpc_urls
        } else if testnet {
            vec![DEFAULT_ELECTRUM_RPC_URL_TESTNET.to_string()]
        } else {
            vec![DEFAULT_ELECTRUM_RPC_URL.to_string()]
        };

        let bitcoin_target_block = if let Some(target_block) = self.bitcoin_target_block {
            target_block
        } else if testnet {
            DEFAULT_BITCOIN_CONFIRMATION_TARGET_TESTNET
        } else {
            DEFAULT_BITCOIN_CONFIRMATION_TARGET
        };

        Ok((bitcoin_electrum_rpc_urls, bitcoin_target_block))
    }
}

#[derive(structopt::StructOpt, Debug)]
pub struct Tor {
    #[structopt(
        long = "enable-tor",
        help = "Bootstrap a tor client and use it for all libp2p connections"
    )]
    pub enable_tor: bool,
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
    // this test is very long, however it just checks that various CLI arguments sets the
    // internal Context and Request properly. It is unlikely to fail and splitting it in various
    // tests would require to run the tests sequantially which is very slow (due to the context
    // need to access files like the Bitcoin wallet).

    /*

    use super::*;

    use crate::cli::api::api_test::*;
    use crate::cli::api::Config;
    use crate::monero::monero_address::MoneroAddressNetworkMismatch;

    const BINARY_NAME: &str = "swap";
    const ARGS_DATA_DIR: &str = "/tmp/dir/";

    TODO: This test doesn't work anymore since the Request struct has been removed. We need to find another way to test the CLI arguments.
    #[tokio::test]
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

        let expected_config = Config::default(is_testnet, None, debug, json);

        let actual_config = match args {
            ParseResult::Context(context, request) => context.config.clone(),
            _ => panic!("Couldn't parse result"),
        };

        assert_eq!(actual_config, expected_config);

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
     */
}
