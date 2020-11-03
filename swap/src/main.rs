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
use std::{io, io::Write, process, sync::Arc};
use structopt::StructOpt;
use swap::{
    alice,
    alice::Alice,
    bitcoin::Wallet,
    bob,
    bob::Bob,
    network::transport::{build, build_tor, SwapTransport},
    Cmd, Rsp, SwapAmounts,
};
use tracing::info;
use url::Url;
use xmr_btc::bitcoin::{BroadcastSignedTransaction, BuildTxLockPsbt, SignTxLock};

mod cli;
mod trace;

use cli::Options;
use swap::{alice, bitcoin, bob, monero, Cmd, Rsp, SwapAmounts};

// TODO: Add root seed file instead of generating new seed each run.

#[tokio::main]
async fn main() -> Result<()> {
    let opt: Options = Options::from_args();

    trace::init_tracing(LevelFilter::Debug)?;

    match opt {
        Options::Alice {
            bitcoind_url: url,
            listen_addr,
            tor_port,
            ..
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
                    let transport = build_tor(local_key_pair, addr.clone(), tor_port)?;
                    (addr, Some(ac), transport)
                }
                None => {
                    let transport = build(local_key_pair)?;
                    (listen_addr, None, transport)
                }
            };

            let bitcoin_wallet = bitcoin::Wallet::new("alice", &url)
                .await
                .expect("failed to create bitcoin wallet");
            let bitcoin_wallet = Arc::new(bitcoin_wallet);

            let monero_wallet = Arc::new(monero::Wallet::localhost(MONERO_WALLET_RPC_PORT));

            swap_as_alice(listen_addr, redeem, punish, transport, behaviour).await?;
        }
        Options::Bob {
            alice_addr,
            satoshis,
            bitcoind_url: url,
        } => {
            info!("running swap node as Bob ...");

            let behaviour = Bob::default();
            let local_key_pair = behaviour.identity();

            let transport = build(local_key_pair)?;

            let bitcoin_wallet = Wallet::new("bob", &url)
                .await
                .expect("failed to create bitcoin wallet");

            let monero_wallet = Arc::new(monero::Wallet::localhost(MONERO_WALLET_RPC_PORT));

            swap_as_bob(
                satoshis,
                alice_addr,
                refund,
                bitcoin_wallet,
                transport,
                behaviour,
            )
            .await?;
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
    addr: Multiaddr,
    redeem: bitcoin::Address,
    punish: bitcoin::Address,
    transport: SwapTransport,
    behaviour: Alice,
) -> Result<()> {
    alice::swap(addr, redeem, punish, transport, behaviour).await
}

async fn swap_as_bob(
    bitcoin_wallet: Arc<swap::bitcoin::Wallet>,
    monero_wallet: Arc<swap::monero::Wallet>,
    sats: u64,
    alice: Multiaddr,
    refund: bitcoin::Address,
    wallet: W,
    transport: SwapTransport,
    behaviour: Bob,
) -> Result<()>
where
    W: BuildTxLockPsbt + SignTxLock + BroadcastSignedTransaction + Send + Sync + 'static,
{
    let (cmd_tx, mut cmd_rx) = mpsc::channel(1);
    let (mut rsp_tx, rsp_rx) = mpsc::channel(1);
    tokio::spawn(bob::swap(
        sats, alice, cmd_tx, rsp_rx, refund, wallet, transport, behaviour,
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
