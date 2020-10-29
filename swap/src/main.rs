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

use anyhow::{bail, Context, Result};
use futures::{channel::mpsc, StreamExt};
use libp2p::core::Multiaddr;
use log::LevelFilter;
use std::{io, io::Write, process, sync::Arc};
use structopt::StructOpt;
use tracing::info;
use url::Url;

mod cli;
mod trace;

use cli::Options;
use swap::{alice, bitcoin, bob, monero, Cmd, Rsp, SwapAmounts};

// TODO: Add root seed file instead of generating new seed each run.

// // TODO: Add a config file with these in it.
// // Alice's address and port until we have a config file.
// pub const LISTEN_PORT: u16 = 9876; // Arbitrarily chosen.
// pub const LISTEN_ADDR: &str = "127.0.0.1";
// pub const BITCOIND_JSON_RPC_URL: &str = "http://127.0.0.1:8332";
//
// #[cfg(feature = "tor")]
// pub const TOR_PORT: u16 = LISTEN_PORT + 1;

#[tokio::main]
async fn main() -> Result<()> {
    let opt: Options = Options::from_args();

    trace::init_tracing(LevelFilter::Debug)?;

    match opt {
        Options::Alice {
            bitcoind_url: url,
            listen_addr,
            listen_port,
            ..
        } => {
            info!("running swap node as Alice ...");

            #[cfg(feature = "tor")]
            let (alice, _ac) = {
                let tor_secret_key = torut::onion::TorSecretKeyV3::generate();
                let onion_address = tor_secret_key
                    .public()
                    .get_onion_address()
                    .get_address_without_dot_onion();
                let onion_address_string = format!("/onion3/{}:{}", onion_address, TOR_PORT);
                let addr: Multiaddr = onion_address_string.parse()?;

                (addr, create_tor_service(tor_secret_key).await?)
            };

            #[cfg(not(feature = "tor"))]
            let alice = {
                let alice: Multiaddr = listen_addr
                    .parse()
                    .expect("failed to parse Alice's address");
            };

            let bitcoin_wallet = bitcoin::Wallet::new("alice", &url)
                .await
                .expect("failed to create bitcoin wallet");
            let bitcoin_wallet = Arc::new(bitcoin_wallet);

            let monero_wallet = Arc::new(monero::Wallet::localhost(MONERO_WALLET_RPC_PORT));

            swap_as_alice(bitcoin_wallet, monero_wallet, alice.clone()).await?;
        }
        Options::Bob {
            alice_addr: alice_address,
            satoshis,
            bitcoind_url: url,
        } => {
            info!("running swap node as Bob ...");

            let bitcoin_wallet = Wallet::new("bob", &url)
                .await
                .expect("failed to create bitcoin wallet");

            let monero_wallet = Arc::new(monero::Wallet::localhost(MONERO_WALLET_RPC_PORT));

            swap_as_bob(satoshis, alice_address, refund, bitcoin_wallet).await?;
        }
    }

    Ok(())
}

#[cfg(feature = "tor")]
async fn create_tor_service(
    tor_secret_key: torut::onion::TorSecretKeyV3,
) -> Result<swap::tor::AuthenticatedConnection> {
    // TODO use configurable ports for tor connection
    let mut authenticated_connection = swap::tor::UnauthenticatedConnection::default()
        .init_authenticated_connection()
        .await?;
    tracing::info!("Tor authenticated.");

    authenticated_connection
        .add_service(TOR_PORT, &tor_secret_key)
        .await?;
    tracing::info!("Tor service added.");

    Ok(authenticated_connection)
}

async fn swap_as_alice(
    bitcoin_wallet: Arc<swap::bitcoin::Wallet>,
    monero_wallet: Arc<swap::monero::Wallet>,
    addr: Multiaddr,
) -> Result<()> {
    #[cfg(not(feature = "tor"))]
    {
        alice::swap(bitcoin_wallet, monero_wallet, addr, None).await
    }
    #[cfg(feature = "tor")]
    {
        alice::swap(bitcoin_wallet, monero_wallet, addr, Some(LISTEN_PORT)).await
    }
}

async fn swap_as_bob(
    bitcoin_wallet: Arc<swap::bitcoin::Wallet>,
    monero_wallet: Arc<swap::monero::Wallet>,
    sats: u64,
    alice: Multiaddr,
) -> Result<()> {
    let (cmd_tx, mut cmd_rx) = mpsc::channel(1);
    let (mut rsp_tx, rsp_rx) = mpsc::channel(1);
    tokio::spawn(bob::swap(
        bitcoin_wallet,
        monero_wallet,
        sats,
        alice,
        cmd_tx,
        rsp_rx,
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

fn multiaddr(s: &str) -> Result<Multiaddr> {
    let addr = s
        .parse()
        .with_context(|| format!("failed to parse multiaddr: {}", s))?;
    Ok(addr)
}
