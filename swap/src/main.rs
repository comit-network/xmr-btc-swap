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
use cli::Options;
use futures::{channel::mpsc, StreamExt};
use libp2p::Multiaddr;
use log::LevelFilter;
use std::{io, io::Write, process};
use structopt::StructOpt;
use swap::{alice, bitcoin::Wallet, bob, Cmd, Rsp, SwapAmounts};
use tracing::info;
use url::Url;
use xmr_btc::bitcoin::{BroadcastSignedTransaction, BuildTxLockPsbt, SignTxLock};

mod cli;
mod trace;

// TODO: Add root seed file instead of generating new seed each run.
// TODO: Remove all instances of the todo! macro

// TODO: Add a config file with these in it.
// Alice's address and port until we have a config file.
pub const PORT: u16 = 9876; // Arbitrarily chosen.
pub const ADDR: &str = "127.0.0.1";
pub const BITCOIND_JSON_RPC_URL: &str = "http://127.0.0.1:8332";

#[cfg(feature = "tor")]
use swap::tor::{AuthenticatedConnection, UnauthenticatedConnection};
#[cfg(feature = "tor")]
use torut::onion::TorSecretKeyV3;
#[cfg(feature = "tor")]
pub const TOR_PORT: u16 = PORT + 1;

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Options::from_args();

    trace::init_tracing(LevelFilter::Debug)?;

    #[cfg(feature = "tor")]
    let (addr, _ac) = {
        let tor_secret_key = TorSecretKeyV3::generate();
        let onion_address = tor_secret_key
            .public()
            .get_onion_address()
            .get_address_without_dot_onion();
        (
            format!("/onion3/{}:{}", onion_address, TOR_PORT),
            create_tor_service(tor_secret_key).await?,
        )
    };
    #[cfg(not(feature = "tor"))]
    let addr = format!("/ip4/{}/tcp/{}", ADDR, PORT);

    let alice: Multiaddr = addr.parse().expect("failed to parse Alice's address");

    if opt.as_alice {
        info!("running swap node as Alice ...");

        if opt.piconeros.is_some() || opt.satoshis.is_some() {
            bail!("Alice cannot set the amount to swap via the cli");
        }

        let url = Url::parse(BITCOIND_JSON_RPC_URL).expect("failed to parse url");
        let bitcoin_wallet = Wallet::new("alice", &url)
            .await
            .expect("failed to create bitcoin wallet");

        let redeem = bitcoin_wallet
            .new_address()
            .await
            .expect("failed to get new redeem address");
        let punish = bitcoin_wallet
            .new_address()
            .await
            .expect("failed to get new punish address");

        swap_as_alice(alice.clone(), redeem, punish).await?;
    } else {
        info!("running swap node as Bob ...");

        let alice_address = match opt.alice_address {
            Some(addr) => addr,
            None => bail!("Address required to dial"),
        };
        let alice_address = multiaddr(&alice_address)?;

        let url = Url::parse(BITCOIND_JSON_RPC_URL).expect("failed to parse url");
        let bitcoin_wallet = Wallet::new("bob", &url)
            .await
            .expect("failed to create bitcoin wallet");

        let refund = bitcoin_wallet
            .new_address()
            .await
            .expect("failed to get new address");

        match (opt.piconeros, opt.satoshis) {
            (Some(_), Some(_)) => bail!("Please supply only a single amount to swap"),
            (None, None) => bail!("Please supply an amount to swap"),
            (Some(_picos), _) => todo!("support starting with picos"),
            (None, Some(sats)) => {
                swap_as_bob(sats, alice_address, refund, bitcoin_wallet).await?;
            }
        };
    }

    Ok(())
}

#[cfg(feature = "tor")]
async fn create_tor_service(tor_secret_key: TorSecretKeyV3) -> Result<AuthenticatedConnection> {
    // todo use configurable ports for tor connection
    let mut authenticated_connection = UnauthenticatedConnection::default()
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
    addr: Multiaddr,
    redeem: bitcoin::Address,
    punish: bitcoin::Address,
) -> Result<()> {
    #[cfg(not(feature = "tor"))]
    {
        alice::swap(addr, None, redeem, punish).await
    }
    #[cfg(feature = "tor")]
    {
        alice::swap(addr, Some(PORT), redeem, punish).await
    }
}

async fn swap_as_bob<W>(
    sats: u64,
    alice: Multiaddr,
    refund: bitcoin::Address,
    wallet: W,
) -> Result<()>
where
    W: BuildTxLockPsbt + SignTxLock + BroadcastSignedTransaction + Send + Sync + 'static,
{
    let (cmd_tx, mut cmd_rx) = mpsc::channel(1);
    let (mut rsp_tx, rsp_rx) = mpsc::channel(1);
    tokio::spawn(bob::swap(sats, alice, cmd_tx, rsp_rx, refund, wallet));

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
