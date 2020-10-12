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

mod cli;

use anyhow::{bail, Result};
use structopt::StructOpt;

use cli::Options;
use swap::{alice, bob, SwapParams};
use xmr_btc::monero;

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Options::from_args();

    if opt.as_alice {
        swap_as_alice(opt)?;
    } else {
        swap_as_bob(opt).await?;
    }

    Ok(())
}

fn swap_as_alice(opt: Options) -> Result<()> {
    println!("running swap as Alice ...");

    if opt.piconeros.is_some() || opt.satoshis.is_some() {
        bail!("Alice cannot set the amount to swap via the cli");
    }

    let _ = alice::swap()?;

    Ok(())
}

async fn swap_as_bob(opt: Options) -> Result<()> {
    println!("running swap as Bob ...");

    match (opt.piconeros, opt.satoshis) {
        (Some(_), Some(_)) => bail!("Please supply only a single amount to swap"),
        (None, None) => bail!("Please supply an amount to swap"),
        _ => {}
    }

    assert_connection_to_alice()?;

    let params = match (opt.piconeros, opt.satoshis) {
        (Some(picos), _) => {
            let xmr = monero::Amount::from_piconero(picos);
            let params = bob::request_swap_xmr(xmr).await?;

            confirm_amounts_with_user(params)?
        }
        (None, Some(sats)) => {
            let btc = bitcoin::Amount::from_sat(sats);
            let params = bob::request_swap_btc(btc).await?;

            confirm_amounts_with_user(params)?
        }
        _ => unreachable!("error path done above"),
    };

    bob::swap(params)
}

fn assert_connection_to_alice() -> Result<()> {
    todo!("assert connection to Alice")
}

fn confirm_amounts_with_user(_: SwapParams) -> Result<SwapParams> {
    todo!("get user confirmation");
}
