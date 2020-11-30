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
use futures::{channel::mpsc, StreamExt};
use libp2p::Multiaddr;
use log::LevelFilter;
use prettytable::{row, Table};
use std::{io, io::Write, process, sync::Arc};
use structopt::StructOpt;
use swap::{
    alice::{self, Alice},
    bitcoin,
    bob::{self, Bob},
    monero,
    network::transport::{build, build_tor, SwapTransport},
    recover::recover,
    Cmd, Rsp, SwapAmounts,
};
use tracing::info;

#[macro_use]
extern crate prettytable;

mod cli;
mod trace;

use cli::Options;
use swap::storage::Database;

// TODO: Add root seed file instead of generating new seed each run.

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Options::from_args();

    trace::init_tracing(LevelFilter::Debug)?;

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

            let behaviour = Alice::default();
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

            let bitcoin_wallet = bitcoin::Wallet::new("alice", bitcoind_url)
                .await
                .expect("failed to create bitcoin wallet");
            let bitcoin_wallet = Arc::new(bitcoin_wallet);

            let monero_wallet = Arc::new(monero::Wallet::new(monerod_url));

            swap_as_alice(
                bitcoin_wallet,
                monero_wallet,
                db,
                listen_addr,
                transport,
                behaviour,
            )
            .await?;
        }
        Options::Bob {
            alice_addr,
            satoshis,
            bitcoind_url,
            monerod_url,
            tor,
        } => {
            info!("running swap node as Bob ...");

            let behaviour = Bob::default();
            let local_key_pair = behaviour.identity();

            let transport = match tor {
                true => build_tor(local_key_pair, None)?,
                false => build(local_key_pair)?,
            };

            let bitcoin_wallet = bitcoin::Wallet::new("bob", bitcoind_url)
                .await
                .expect("failed to create bitcoin wallet");
            let bitcoin_wallet = Arc::new(bitcoin_wallet);

            let monero_wallet = Arc::new(monero::Wallet::new(monerod_url));

            swap_as_bob(
                bitcoin_wallet,
                monero_wallet,
                db,
                satoshis,
                alice_addr,
                transport,
                behaviour,
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

async fn swap_as_alice(
    bitcoin_wallet: Arc<swap::bitcoin::Wallet>,
    monero_wallet: Arc<swap::monero::Wallet>,
    db: Database,
    addr: Multiaddr,
    transport: SwapTransport,
    behaviour: Alice,
) -> Result<()> {
    alice::swap(
        bitcoin_wallet,
        monero_wallet,
        db,
        addr,
        transport,
        behaviour,
    )
    .await
}

async fn swap_as_bob(
    bitcoin_wallet: Arc<swap::bitcoin::Wallet>,
    monero_wallet: Arc<swap::monero::Wallet>,
    db: Database,
    sats: u64,
    alice: Multiaddr,
    transport: SwapTransport,
    behaviour: Bob,
) -> Result<()> {
    let (cmd_tx, mut cmd_rx) = mpsc::channel(1);
    let (mut rsp_tx, rsp_rx) = mpsc::channel(1);
    tokio::spawn(bob::swap(
        bitcoin_wallet,
        monero_wallet,
        db,
        sats,
        alice,
        cmd_tx,
        rsp_rx,
        transport,
        behaviour,
    ));

    loop {
        let read = cmd_rx.next().await;
        match read {
            Some(cmd) => match cmd {
                Cmd::VerifyAmounts(p) => {
                    let rsp = verify(p);
                    rsp_tx.try_send(rsp)?;
                    if rsp == Rsp::Abort {
                        process::exit(0);
                    }
                }
            },
            None => {
                info!("Channel closed from other end");
                return Ok(());
            }
        }
    }
}

fn verify(amounts: SwapAmounts) -> Rsp {
    let mut s = String::new();
    println!("Got rate from Alice for XMR/BTC swap\n");
    println!("{}", amounts);
    print!("Would you like to continue with this swap [y/N]: ");

    let _ = io::stdout().flush();
    io::stdin()
        .read_line(&mut s)
        .expect("Did not enter a correct string");

    if let Some('\n') = s.chars().next_back() {
        s.pop();
    }
    if let Some('\r') = s.chars().next_back() {
        s.pop();
    }

    if !is_yes(&s) {
        println!("No worries, try again later - Alice updates her rate regularly");
        return Rsp::Abort;
    }

    Rsp::VerifiedAmounts
}

fn is_yes(s: &str) -> bool {
    matches!(s, "y" | "Y" | "yes" | "YES" | "Yes")
}
