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
use prettytable::{row, Table};
use rand::rngs::OsRng;
use std::sync::Arc;
use structopt::StructOpt;
use swap::{
    alice, alice::swap::AliceState, bitcoin, bob, bob::swap::BobState, cli::Options, monero,
    network::transport::build, recover::recover, storage::Database, trace::init_tracing,
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

    // This currently creates the directory if it's not there in the first place
    let db = Database::open(std::path::Path::new("./.swap-db/")).unwrap();
    let config = Config::mainnet();

    match opt {
        Options::SellXmr {
            bitcoind_url,
            bitcoin_wallet_name,
            monero_wallet_rpc_url,
            listen_addr,
            send_monero,
            receive_bitcoin,
        } => {
            info!("running swap node as Alice ...");

            let bitcoin_wallet = bitcoin::Wallet::new(
                bitcoin_wallet_name.as_str(),
                bitcoind_url,
                config.bitcoin_network,
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
                "Connection to Monero wallet succeeded, balance: {}",
                monero_balance
            );
            let monero_wallet = Arc::new(monero_wallet);

            let amounts = SwapAmounts {
                btc: receive_bitcoin,
                xmr: send_monero,
            };

            let (alice_state, alice_behaviour) = {
                let rng = &mut OsRng;
                let a = bitcoin::SecretKey::new_random(rng);
                let s_a = cross_curve_dleq::Scalar::random(rng);
                let v_a = xmr_btc::monero::PrivateViewKey::new_random(rng);
                let redeem_address = bitcoin_wallet.as_ref().new_address().await.unwrap();
                let punish_address = redeem_address.clone();
                let state0 = State0::new(
                    a,
                    s_a,
                    v_a,
                    amounts.btc,
                    amounts.xmr,
                    config.bitcoin_refund_timelock,
                    config.bitcoin_punish_timelock,
                    redeem_address,
                    punish_address,
                );

                (
                    AliceState::Started {
                        amounts,
                        state0: state0.clone(),
                    },
                    alice::Behaviour::new(state0),
                )
            };

            let alice_peer_id = alice_behaviour.peer_id();
            info!(
                "Alice Peer ID (to be used by Bob to dial her): {}",
                alice_peer_id
            );

            let alice_transport = build(alice_behaviour.identity())?;

            let (mut event_loop, handle) =
                alice::event_loop::EventLoop::new(alice_transport, alice_behaviour, listen_addr)?;

            let swap = alice::swap::swap(
                alice_state,
                handle,
                bitcoin_wallet.clone(),
                monero_wallet.clone(),
                config,
            );

            let _event_loop = tokio::spawn(async move { event_loop.run().await });
            swap.await?;
        }
        Options::BuyXmr {
            alice_addr,
            alice_peer_id: _,
            bitcoind_url,
            bitcoin_wallet_name,
            monero_wallet_rpc_url,
            send_bitcoin,
            receive_monero,
        } => {
            info!("running swap node as Bob ...");

            let bob_behaviour = bob::Behaviour::default();
            let bob_transport = build(bob_behaviour.identity())?;

            let bitcoin_wallet = bitcoin::Wallet::new(
                bitcoin_wallet_name.as_str(),
                bitcoind_url,
                config.bitcoin_network,
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
                "Connection to Monero wallet succeeded, balance: {}",
                monero_balance
            );
            let monero_wallet = Arc::new(monero_wallet);

            let refund_address = bitcoin_wallet.new_address().await.unwrap();
            let state0 = xmr_btc::bob::State0::new(
                &mut OsRng,
                send_bitcoin,
                receive_monero,
                config.bitcoin_refund_timelock,
                config.bitcoin_punish_timelock,
                refund_address,
            );

            let amounts = SwapAmounts {
                btc: send_bitcoin,
                xmr: receive_monero,
            };

            let bob_state = BobState::Started {
                state0,
                amounts,
                addr: alice_addr,
            };

            let (event_loop, handle) =
                bob::event_loop::EventLoop::new(bob_transport, bob_behaviour).unwrap();

            let swap = bob::swap::swap(
                bob_state,
                handle,
                db,
                bitcoin_wallet.clone(),
                monero_wallet.clone(),
                OsRng,
                Uuid::new_v4(),
            );

            let _event_loop = tokio::spawn(async move { event_loop.run().await });
            swap.await?;
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
                config.bitcoin_network,
            )
            .await
            .expect("failed to create bitcoin wallet");
            let monero_wallet = monero::Wallet::new(monerod_url);

            recover(bitcoin_wallet, monero_wallet, state).await?;
        }
    }

    Ok(())
}
