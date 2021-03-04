use anyhow::{Context, Result};
use libp2p::core::Multiaddr;
use libp2p::PeerId;
use std::path::PathBuf;
use std::str::FromStr;
use structopt::clap::AppSettings;
use structopt::StructOpt;
use uuid::Uuid;

pub const DEFAULT_ALICE_MULTIADDR: &str = "/dns4/xmr-btc-asb.coblox.tech/tcp/9876";
pub const DEFAULT_ALICE_PEER_ID: &str = "12D3KooWCdMKjesXMJz1SiZ7HgotrxuqhQJbP5sgBm2BwP1cqThi";

pub struct CliExecutionParams {
    pub config: Option<PathBuf>,
    pub debug: bool,
    pub command: Command,
}

impl CliExecutionParams {
    pub fn from_args() -> Self {
        let matches = Arguments::clap()
            .setting(AppSettings::SubcommandsNegateReqs)
            .setting(AppSettings::ArgsNegateSubcommands)
            .get_matches();
        if matches.subcommand_name().is_none() {
            let args = Arguments::from_clap(&matches);
            Self {
                config: args.standard_opts.config,
                debug: args.standard_opts.debug,
                command: Command::BuyXmr {
                    receive_monero_address: args.receive_monero_address,
                    alice_peer_id: args.alice_connection.alice_peer_id,
                    alice_addr: args.alice_connection.alice_addr,
                },
            }
        } else {
            let sub_command: SubCommand = SubCommand::from_clap(&matches);
            match sub_command {
                SubCommand::History { debug } => CliExecutionParams {
                    config: None,
                    debug,
                    command: Command::History,
                },
                SubCommand::Cancel {
                    swap_id,
                    force,
                    standard_opts: StandardOpts { config, debug },
                } => CliExecutionParams {
                    config,
                    debug,
                    command: Command::Cancel { swap_id, force },
                },
                SubCommand::Refund {
                    swap_id,
                    force,
                    standard_opts: StandardOpts { config, debug },
                } => CliExecutionParams {
                    config,
                    debug,
                    command: Command::Refund { swap_id, force },
                },
                SubCommand::Resume {
                    receive_monero_address,
                    swap_id,
                    alice_connection:
                        AliceConnection {
                            alice_peer_id,
                            alice_addr,
                        },
                    standard_opts: StandardOpts { config, debug },
                } => CliExecutionParams {
                    config,
                    debug,
                    command: Command::Resume {
                        receive_monero_address,
                        swap_id,
                        alice_peer_id,
                        alice_addr,
                    },
                },
            }
        }
    }
}

#[allow(clippy::large_enum_variant)]
pub enum Command {
    BuyXmr {
        receive_monero_address: monero::Address,
        alice_peer_id: PeerId,
        alice_addr: Multiaddr,
    },
    History,
    Resume {
        receive_monero_address: monero::Address,
        swap_id: Uuid,
        alice_peer_id: PeerId,
        alice_addr: Multiaddr,
    },
    Cancel {
        swap_id: Uuid,
        force: bool,
    },
    Refund {
        swap_id: Uuid,
        force: bool,
    },
}

#[derive(structopt::StructOpt, Debug)]
pub struct Arguments {
    #[structopt(long = "receive-address")]
    receive_monero_address: monero::Address,

    #[structopt(flatten)]
    standard_opts: StandardOpts,

    #[structopt(flatten)]
    alice_connection: AliceConnection,

    #[structopt(subcommand)]
    pub sub_command: Option<SubCommand>,
}

#[allow(clippy::large_enum_variant)]
#[derive(structopt::StructOpt, Debug)]
#[structopt(name = "xmr_btc-swap", about = "XMR BTC atomic swap")]
pub enum SubCommand {
    History {
        #[structopt(long, help = "Activate debug logging.")]
        debug: bool,
    },
    Resume {
        #[structopt(long = "receive-address", parse(try_from_str = parse_monero_address))]
        receive_monero_address: monero::Address,

        #[structopt(long = "swap-id")]
        swap_id: Uuid,

        // TODO: Remove Alice peer-id/address, it should be saved in the database when running swap
        // and loaded from the database when running resume/cancel/refund
        #[structopt(flatten)]
        alice_connection: AliceConnection,

        #[structopt(flatten)]
        standard_opts: StandardOpts,
    },
    Cancel {
        #[structopt(long = "swap-id")]
        swap_id: Uuid,

        #[structopt(short, long)]
        force: bool,

        #[structopt(flatten)]
        standard_opts: StandardOpts,
    },
    Refund {
        #[structopt(long = "swap-id")]
        swap_id: Uuid,

        #[structopt(short, long)]
        force: bool,

        #[structopt(flatten)]
        standard_opts: StandardOpts,
    },
}

#[derive(structopt::StructOpt, Debug)]
pub struct StandardOpts {
    #[structopt(
        long = "config",
        help = "Provide a custom path to the configuration file. The configuration file must be a toml file.",
        parse(from_os_str)
    )]
    config: Option<PathBuf>,

    #[structopt(long, help = "Activate debug logging.")]
    debug: bool,
}

#[derive(structopt::StructOpt, Debug)]
pub struct AliceConnection {
    #[structopt(long = "connect-peer-id", default_value = DEFAULT_ALICE_PEER_ID)]
    alice_peer_id: PeerId,

    #[structopt(long = "connect-addr", default_value = DEFAULT_ALICE_MULTIADDR)]
    alice_addr: Multiaddr,
}

fn parse_monero_address(s: &str) -> Result<monero::Address> {
    monero::Address::from_str(s).with_context(|| {
        format!(
            "Failed to parse {} as a monero address, please make sure it is a valid address",
            s
        )
    })
}

#[cfg(test)]
mod tests {
    use crate::cli::command::{DEFAULT_ALICE_MULTIADDR, DEFAULT_ALICE_PEER_ID};
    use libp2p::core::Multiaddr;
    use libp2p::PeerId;

    #[test]
    fn parse_default_alice_peer_id_success() {
        DEFAULT_ALICE_PEER_ID
            .parse::<PeerId>()
            .expect("default alice peer id str is a valid PeerId");
    }

    #[test]
    fn parse_default_alice_multiaddr_success() {
        DEFAULT_ALICE_MULTIADDR
            .parse::<Multiaddr>()
            .expect("default alice multiaddr str is a valid Multiaddr>");
    }
}
