use anyhow::{Context, Result};

#[tokio::main]
async fn main() -> Result<()> {
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt().with_env_filter("debug").finish(),
    )?;

    let mut ticker = swap::kraken::connect().context("Failed to connect to kraken")?;

    loop {
        match ticker.wait_for_update().await? {
            Ok(rate) => println!("Rate update: {}", rate),
            Err(e) => println!("Error: {:#}", e),
        }
    }
}
