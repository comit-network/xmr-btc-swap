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
use std::sync::Arc;
use structopt::StructOpt;
use swap::{
    alice, bitcoin, bob,
    cli::Options,
    monero,
    network::transport::{build, build_tor},
    recover::recover,
    storage::Database,
};
use tracing::info;

#[macro_use]
extern crate prettytable;

// TODO: Add root seed file instead of generating new seed each run.

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Options::from_args();

    // This currently creates the directory if it's not there in the first place
    let db = Database::open(std::path::Path::new("./.swap-db/")).unwrap();

    match opt {
        Options::Alice {
            bitcoind_url,
            monerod_url,
            listen_addr,
            tor_port,
        } => {
            info!("running swap node as Alice ...");

            let behaviour = alice::Behaviour::default();
            let local_key_pair = behaviour.identity();

            let (_listen_addr, _ac, _transport) = match tor_port {
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

            let bitcoin_wallet = bitcoin::Wallet::new("alice", bitcoind_url)
                .await
                .expect("failed to create bitcoin wallet");
            let _bitcoin_wallet = Arc::new(bitcoin_wallet);

            let _monero_wallet = Arc::new(monero::Wallet::new(monerod_url));

            // TODO: Call swap function
        }
        Options::Bob {
            alice_addr: _,
            satoshis: _,
            bitcoind_url,
            monerod_url,
            tor,
        } => {
            info!("running swap node as Bob ...");

            let behaviour = bob::Behaviour::default();
            let local_key_pair = behaviour.identity();

            let _transport = match tor {
                true => build_tor(local_key_pair, None)?,
                false => build(local_key_pair)?,
            };

            let bitcoin_wallet = bitcoin::Wallet::new("bob", bitcoind_url)
                .await
                .expect("failed to create bitcoin wallet");
            let _bitcoin_wallet = Arc::new(bitcoin_wallet);

            let _monero_wallet = Arc::new(monero::Wallet::new(monerod_url));

            // TODO: Call swap function
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
        } => {
            let state = db.get_state(swap_id)?;
            let bitcoin_wallet = bitcoin::Wallet::new("bob", bitcoind_url)
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
