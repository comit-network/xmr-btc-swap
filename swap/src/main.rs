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

use anyhow::{bail, Result};
use futures::{channel::mpsc, StreamExt};
use libp2p::Multiaddr;
use log::LevelFilter;
use std::{io, io::Write, process};
use structopt::StructOpt;
use tracing::info;
mod cli;
mod trace;

use cli::Options;
use swap::{alice, bob, Cmd, Rsp, SwapParams};

// TODO: Add root seed file instead of generating new seed each run.

// Alice's address and port until we have a config file.
pub const PORT: u16 = 9876; // Arbitrarily chosen.
pub const ADDR: &str = "127.0.0.1";

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Options::from_args();

    trace::init_tracing(LevelFilter::Debug)?;

    let addr = format!("/ip4/{}/tcp/{}", ADDR, PORT);
    let alice_addr: Multiaddr = addr.parse().expect("failed to parse Alice's address");

    if opt.as_alice {
        info!("running swap node as Alice ...");

        if opt.piconeros.is_some() || opt.satoshis.is_some() {
            bail!("Alice cannot set the amount to swap via the cli");
        }

        swap_as_alice(alice_addr).await?;
    } else {
        info!("running swap node as Bob ...");

        match (opt.piconeros, opt.satoshis) {
            (Some(_), Some(_)) => bail!("Please supply only a single amount to swap"),
            (None, None) => bail!("Please supply an amount to swap"),
            (Some(_picos), _) => todo!("support starting with picos"),
            (None, Some(sats)) => {
                swap_as_bob(sats, alice_addr).await?;
            }
        };
    }

    Ok(())
}

async fn swap_as_alice(addr: Multiaddr) -> Result<()> {
    alice::swap(addr).await
}

async fn swap_as_bob(sats: u64, addr: Multiaddr) -> Result<()> {
    let (cmd_tx, mut cmd_rx) = mpsc::channel(1);
    let (mut rsp_tx, rsp_rx) = mpsc::channel(1);
    tokio::spawn(bob::swap(sats, addr, cmd_tx, rsp_rx));
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

fn verify(p: SwapParams) -> Rsp {
    let mut s = String::new();
    println!("Got rate from Alice for XMR/BTC swap\n");
    println!("{}", p);
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

    Rsp::Verified
}

fn is_yes(s: &str) -> bool {
    matches!(s, "y" | "Y" | "yes" | "YES" | "Yes")
}
