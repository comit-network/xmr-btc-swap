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
#![allow(non_snake_case)]

use anyhow::{bail, Context, Result};
use libp2p::{core::Multiaddr, PeerId};
use prettytable::{row, Table};
use rand::rngs::OsRng;
use std::sync::Arc;
use structopt::StructOpt;
use swap::{
    alice,
    alice::swap::AliceState,
    bitcoin, bob,
    bob::swap::BobState,
    cli::{Command, Options, Resume},
    database::Database,
    monero,
    network::transport::build,
    state::Swap,
    trace::init_tracing,
    SwapAmounts,
};
use tracing::{info, log::LevelFilter};
use uuid::Uuid;
use xmr_btc::{alice::State0, config::Config, cross_curve_dleq};

#[macro_use]
extern crate prettytable;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing(LevelFilter::Trace).expect("initialize tracing");

    let opt = Options::from_args();

    let config = Config::mainnet();

    info!("Database: {}", opt.db_path);
    let db = Database::open(std::path::Path::new(opt.db_path.as_str()))
        .context("Could not open database")?;

    match opt.cmd {
        Command::SellXmr {
            bitcoind_url,
            bitcoin_wallet_name,
            monero_wallet_rpc_url,
            listen_addr,
            send_monero,
            receive_bitcoin,
        } => {
            let (bitcoin_wallet, monero_wallet) = setup_wallets(
                bitcoind_url,
                bitcoin_wallet_name.as_str(),
                monero_wallet_rpc_url,
                config,
            )
            .await?;

            let amounts = SwapAmounts {
                btc: receive_bitcoin,
                xmr: send_monero,
            };

            let alice_state = {
                let rng = &mut OsRng;
                let a = bitcoin::SecretKey::new_random(rng);
                let s_a = cross_curve_dleq::Scalar::random(rng);
                let v_a = xmr_btc::monero::PrivateViewKey::new_random(rng);
                let redeem_address = bitcoin_wallet.as_ref().new_address().await?;
                let punish_address = redeem_address.clone();
                let state0 = State0::new(
                    a,
                    s_a,
                    v_a,
                    amounts.btc,
                    amounts.xmr,
                    config.bitcoin_cancel_timelock,
                    config.bitcoin_punish_timelock,
                    redeem_address,
                    punish_address,
                );

                AliceState::Started { amounts, state0 }
            };

            let swap_id = Uuid::new_v4();
            info!(
                "Swap sending {} and receiving {} started with ID {}",
                send_monero, receive_bitcoin, swap_id
            );

            alice_swap(
                swap_id,
                alice_state,
                listen_addr,
                bitcoin_wallet,
                monero_wallet,
                config,
                db,
            )
            .await?;
        }
        Command::BuyXmr {
            alice_peer_id,
            alice_addr,
            bitcoind_url,
            bitcoin_wallet_name,
            monero_wallet_rpc_url,
            send_bitcoin,
            receive_monero,
        } => {
            let (bitcoin_wallet, monero_wallet) = setup_wallets(
                bitcoind_url,
                bitcoin_wallet_name.as_str(),
                monero_wallet_rpc_url,
                config,
            )
            .await?;

            let refund_address = bitcoin_wallet.new_address().await?;
            let state0 = xmr_btc::bob::State0::new(
                &mut OsRng,
                send_bitcoin,
                receive_monero,
                config.bitcoin_cancel_timelock,
                config.bitcoin_punish_timelock,
                refund_address,
            );

            let amounts = SwapAmounts {
                btc: send_bitcoin,
                xmr: receive_monero,
            };

            let bob_state = BobState::Started { state0, amounts };

            let swap_id = Uuid::new_v4();
            info!(
                "Swap sending {} and receiving {} started with ID {}",
                send_bitcoin, receive_monero, swap_id
            );

            bob_swap(
                swap_id,
                bob_state,
                bitcoin_wallet,
                monero_wallet,
                db,
                alice_peer_id,
                alice_addr,
            )
            .await?;
        }
        Command::History => {
            let mut table = Table::new();

            table.add_row(row!["SWAP ID", "STATE"]);

            for (swap_id, state) in db.all()? {
                table.add_row(row![swap_id, state]);
            }

            // Print the table to stdout
            table.printstd();
        }
        Command::Resume(Resume::SellXmr {
            swap_id,
            bitcoind_url,
            bitcoin_wallet_name,
            monero_wallet_rpc_url,
            listen_addr,
        }) => {
            let db_state = if let Swap::Alice(db_state) = db.get_state(swap_id)? {
                db_state
            } else {
                bail!("Swap {} is not sell xmr.", swap_id)
            };

            let (bitcoin_wallet, monero_wallet) = setup_wallets(
                bitcoind_url,
                bitcoin_wallet_name.as_str(),
                monero_wallet_rpc_url,
                config,
            )
            .await?;
            alice_swap(
                swap_id,
                db_state.into(),
                listen_addr,
                bitcoin_wallet,
                monero_wallet,
                config,
                db,
            )
            .await?;
        }
        Command::Resume(Resume::BuyXmr {
            swap_id,
            bitcoind_url,
            bitcoin_wallet_name,
            monero_wallet_rpc_url,
            alice_peer_id,
            alice_addr,
        }) => {
            let db_state = if let Swap::Bob(db_state) = db.get_state(swap_id)? {
                db_state
            } else {
                bail!("Swap {} is not buy xmr.", swap_id)
            };

            let (bitcoin_wallet, monero_wallet) = setup_wallets(
                bitcoind_url,
                bitcoin_wallet_name.as_str(),
                monero_wallet_rpc_url,
                config,
            )
            .await?;
            bob_swap(
                swap_id,
                db_state.into(),
                bitcoin_wallet,
                monero_wallet,
                db,
                alice_peer_id,
                alice_addr,
            )
            .await?;
        }
    };

    Ok(())
}

async fn setup_wallets(
    bitcoind_url: url::Url,
    bitcoin_wallet_name: &str,
    monero_wallet_rpc_url: url::Url,
    config: Config,
) -> Result<(Arc<bitcoin::Wallet>, Arc<monero::Wallet>)> {
    let bitcoin_wallet =
        bitcoin::Wallet::new(bitcoin_wallet_name, bitcoind_url, config.bitcoin_network).await?;
    let bitcoin_balance = bitcoin_wallet.balance().await?;
    info!(
        "Connection to Bitcoin wallet succeeded, balance: {}",
        bitcoin_balance
    );
    let bitcoin_wallet = Arc::new(bitcoin_wallet);

    let monero_wallet = monero::Wallet::new(monero_wallet_rpc_url);
    let monero_balance = monero_wallet.get_balance().await?;
    info!(
        "Connection to Monero wallet succeeded, balance: {}",
        monero_balance
    );
    let monero_wallet = Arc::new(monero_wallet);

    Ok((bitcoin_wallet, monero_wallet))
}

async fn alice_swap(
    swap_id: Uuid,
    state: AliceState,
    listen_addr: Multiaddr,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,
    config: Config,
    db: Database,
) -> Result<AliceState> {
    let alice_behaviour = alice::Behaviour::default();

    let alice_peer_id = alice_behaviour.peer_id();
    info!("Own Peer-ID: {}", alice_peer_id);

    let alice_transport = build(alice_behaviour.identity())?;

    let (mut event_loop, handle) =
        alice::event_loop::EventLoop::new(alice_transport, alice_behaviour, listen_addr)?;

    let swap = alice::swap::swap(
        state,
        handle,
        bitcoin_wallet.clone(),
        monero_wallet.clone(),
        config,
        swap_id,
        db,
    );

    tokio::spawn(async move { event_loop.run().await });
    swap.await
}

async fn bob_swap(
    swap_id: Uuid,
    state: BobState,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,
    db: Database,
    alice_peer_id: PeerId,
    alice_addr: Multiaddr,
) -> Result<BobState> {
    let bob_behaviour = bob::Behaviour::default();
    let bob_transport = build(bob_behaviour.identity())?;

    let (event_loop, handle) =
        bob::event_loop::EventLoop::new(bob_transport, bob_behaviour, alice_peer_id, alice_addr)?;

    let swap = bob::swap::swap(
        state,
        handle,
        db,
        bitcoin_wallet.clone(),
        monero_wallet.clone(),
        OsRng,
        swap_id,
    );

    tokio::spawn(event_loop.run());
    swap.await
}
