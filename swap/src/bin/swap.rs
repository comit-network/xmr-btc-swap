#![warn(
    unused_extern_crates,
    missing_debug_implementations,
    missing_copy_implementations,
    rust_2018_idioms,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::fallible_impl_from,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::dbg_macro
)]
#![forbid(unsafe_code)]

use anyhow::Result;
use libp2p::Multiaddr;
use prettytable::{row, Table};
use rand::rngs::OsRng;
use std::sync::Arc;
use structopt::StructOpt;
use swap::{
    alice,
    alice::swap::AliceState,
    bitcoin, bob,
    bob::swap::BobState,
    cli::Options,
    monero,
    network::transport::{build, build_tor},
    recover::recover,
    storage::Database,
    SwapAmounts, PUNISH_TIMELOCK, REFUND_TIMELOCK,
};
use tracing::{info, log::LevelFilter, subscriber};
use tracing_log::LogTracer;
use tracing_subscriber::FmtSubscriber;
use uuid::Uuid;
use xmr_btc::{config::Config, cross_curve_dleq};

#[macro_use]
extern crate prettytable;

// TODO: Add root seed file instead of generating new seed each run.

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing(LevelFilter::Trace).expect("initialize tracing");

    let opt = Options::from_args();

    // This currently creates the directory if it's not there in the first place
    let db = Database::open(std::path::Path::new("./.swap-db/")).unwrap();

    let rng = &mut OsRng;

    match opt {
        Options::Alice {
            bitcoind_url,
            bitcoin_wallet_name,
            monero_wallet_rpc_url,
            listen_addr,
            tor_port,
            send_monero,
            receive_bitcoin,
        } => {
            info!("running swap node as Alice ...");

            let behaviour = alice::Behaviour::default();
            let alice_peer_id = behaviour.peer_id().clone();
            info!(
                "Alice Peer ID (to be used by Bob to dial her): {}",
                alice_peer_id
            );
            let local_key_pair = behaviour.identity();

            let (listen_addr, _ac, transport) = match tor_port {
                Some(tor_port) => {
                    let tor_secret_key = torut::onion::TorSecretKeyV3::generate();
                    let onion_address = tor_secret_key
                        .public()
                        .get_onion_address()
                        .get_address_without_dot_onion();
                    let onion_address_string = format!("/onion3/{}:{}", onion_address, tor_port);
                    let addr: Multiaddr = onion_address_string.parse()?;
                    let ac = create_tor_service(tor_secret_key, tor_port).await?;
                    let transport = build_tor(local_key_pair, Some((addr.clone(), tor_port)))?;
                    (addr, Some(ac), transport)
                }
                None => {
                    let transport = build(local_key_pair)?;
                    (listen_addr, None, transport)
                }
            };

            let amounts = SwapAmounts {
                btc: receive_bitcoin,
                xmr: send_monero,
            };

            // TODO: network should be configurable through CLI, defaulting to mainnet
            let bitcoin_wallet = bitcoin::Wallet::new(
                bitcoin_wallet_name.as_str(),
                bitcoind_url,
                ::bitcoin::Network::Bitcoin,
            )
            .await
            .expect("failed to create bitcoin wallet");

            let bitcoin_balance = bitcoin_wallet.balance().await?;
            info!(
                "Connection to Bitcoin wallet succeeded, balance: {}",
                bitcoin_balance
            );
            let bitcoin_wallet = Arc::new(bitcoin_wallet);

            let monero_wallet = monero::Wallet::new(monero_wallet_rpc_url);
            let monero_balance = monero_wallet.get_balance().await?;
            // TODO: impl Display for monero wallet to display proper monero balance
            info!(
                "Connection to Monero wallet succeeded, balance: {:?}",
                monero_balance
            );
            let monero_wallet = Arc::new(monero_wallet);

            let alice_state = {
                let a = bitcoin::SecretKey::new_random(rng);
                let s_a = cross_curve_dleq::Scalar::random(rng);
                let v_a = xmr_btc::monero::PrivateViewKey::new_random(rng);
                AliceState::Started {
                    amounts,
                    a,
                    s_a,
                    v_a,
                }
            };
            let alice_swarm = alice::new_swarm(listen_addr.clone(), transport, behaviour).unwrap();

            alice::swap::swap(
                alice_state,
                alice_swarm,
                bitcoin_wallet.clone(),
                monero_wallet.clone(),
                Config::mainnet(),
            )
            .await?;
        }
        Options::Bob {
            alice_addr,
            alice_peer_id,
            bitcoind_url,
            bitcoin_wallet_name,
            monero_wallet_rpc_url,
            tor,
            send_bitcoin,
            receive_monero,
        } => {
            info!("running swap node as Bob ...");

            let behaviour = bob::Behaviour::default();
            let local_key_pair = behaviour.identity();

            let transport = match tor {
                true => build_tor(local_key_pair, None)?,
                false => build(local_key_pair)?,
            };

            let amounts = SwapAmounts {
                btc: send_bitcoin,
                xmr: receive_monero,
            };

            let bitcoin_wallet = bitcoin::Wallet::new(
                bitcoin_wallet_name.as_str(),
                bitcoind_url,
                ::bitcoin::Network::Bitcoin,
            )
            .await
            .expect("failed to create bitcoin wallet");
            let bitcoin_balance = bitcoin_wallet.balance().await?;
            info!(
                "Connection to Bitcoin wallet succeeded, balance: {}",
                bitcoin_balance
            );
            let bitcoin_wallet = Arc::new(bitcoin_wallet);

            let monero_wallet = monero::Wallet::new(monero_wallet_rpc_url);
            let monero_balance = monero_wallet.get_balance().await?;
            info!(
                "Connection to Monero wallet succeeded, balance: {:?}",
                monero_balance
            );
            let monero_wallet = Arc::new(monero_wallet);

            let refund_address = bitcoin_wallet.new_address().await.unwrap();
            let state0 = xmr_btc::bob::State0::new(
                rng,
                send_bitcoin,
                receive_monero,
                REFUND_TIMELOCK,
                PUNISH_TIMELOCK,
                refund_address,
            );

            let bob_state = BobState::Started {
                state0,
                amounts,
                peer_id: alice_peer_id,
                addr: alice_addr,
            };
            let bob_swarm = bob::new_swarm(transport, behaviour).unwrap();
            bob::swap::swap(
                bob_state,
                bob_swarm,
                db,
                bitcoin_wallet.clone(),
                monero_wallet.clone(),
                OsRng,
                Uuid::new_v4(),
            )
            .await?;
        }
        Options::History => {
            let mut table = Table::new();

            table.add_row(row!["SWAP ID", "STATE"]);

            for (swap_id, state) in db.all()? {
                table.add_row(row![swap_id, state]);
            }

            // Print the table to stdout
            table.printstd();
        }
        Options::Recover {
            swap_id,
            bitcoind_url,
            monerod_url,
            bitcoin_wallet_name,
        } => {
            let state = db.get_state(swap_id)?;
            let bitcoin_wallet = bitcoin::Wallet::new(
                bitcoin_wallet_name.as_ref(),
                bitcoind_url,
                ::bitcoin::Network::Bitcoin,
            )
            .await
            .expect("failed to create bitcoin wallet");
            let monero_wallet = monero::Wallet::new(monerod_url);

            recover(bitcoin_wallet, monero_wallet, state).await?;
        }
    }

    Ok(())
}

async fn create_tor_service(
    tor_secret_key: torut::onion::TorSecretKeyV3,
    tor_port: u16,
) -> Result<swap::tor::AuthenticatedConnection> {
    // TODO use configurable ports for tor connection
    let mut authenticated_connection = swap::tor::UnauthenticatedConnection::default()
        .init_authenticated_connection()
        .await?;
    tracing::info!("Tor authenticated.");

    authenticated_connection
        .add_service(tor_port, &tor_secret_key)
        .await?;
    tracing::info!("Tor service added.");

    Ok(authenticated_connection)
}

pub fn init_tracing(level: log::LevelFilter) -> anyhow::Result<()> {
    if level == LevelFilter::Off {
        return Ok(());
    }

    // We want upstream library log messages, just only at Info level.
    LogTracer::init_with_filter(LevelFilter::Info)?;

    let is_terminal = atty::is(atty::Stream::Stderr);
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(format!(
            "swap={},xmr-btc={},http=info,warp=info",
            level, level
        ))
        .with_writer(std::io::stderr)
        .with_ansi(is_terminal)
        .finish();

    subscriber::set_global_default(subscriber)?;
    info!("Initialized tracing with level: {}", level);

    Ok(())
}
