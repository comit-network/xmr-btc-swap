use anyhow::{Context, Result};

#[tokio::main]
async fn main() -> Result<()> {
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt().with_env_filter("trace").finish(),
    )?;

    let mut ticker = swap::kraken::connect()
        .await
        .context("Failed to connect to kraken")?;

    loop {
        match ticker.wait_for_update().await? {
            Ok(rate) => println!("Rate update: {}", rate),
            Err(e) => println!("Error: {:#}", e),
        }
    }
}
