use anyhow::{bail, Result};
use rustyline::{error::ReadlineError, Editor};
use structopt::StructOpt;
use xmr_btc::cli::Opt;

fn run_app() -> Result<()> {
    let opt = Opt::from_args();

    if opt.maker {
        run_maker()?
    } else if opt.taker {
        let addr = match opt.address {
            Some(addr) => addr,
            None => bail!("Maker address is required"),
        };

        run_taker(addr)?
    } else {
        bail!("Invalid argument")
    }

    Ok(())
}

fn main() {
    std::process::exit(match run_app() {
        Ok(_) => 0,
        Err(err) => {
            eprintln!("error: {:?}", err);
            1
        }
    });
}

fn run_maker() -> Result<()> {
    // wait for a taker to connect and auto trade
    println!("Usually we would now wait for takers to connect");
    Ok(())
}

fn run_taker(_addr: String) -> Result<()> {
    let mut rl = Editor::<()>::new();

    // connect to maker and get rate

    let readline = rl.readline("Received order from maker: 1 BTC for 42 XMR. Take it or leave it? Hit <enter> to accept or CTRL-C to decline and quit.\n ");
    match readline {
        Ok(line) => {
            rl.add_history_entry(line.as_str());
            println!("Accepting trade doing the swap");
            Ok(())
        }
        Err(ReadlineError::Interrupted) => {
            println!("Trade canceled. Terminating application");
            Ok(())
        }
        Err(ReadlineError::Eof) => {
            println!("Trade canceled. Terminating application");
            Ok(())
        }
        Err(err) => bail!("Error: {:?}", err),
    }
}
