use anyhow::{Context, Result};
use url::Url;

#[tokio::main]
async fn main() -> Result<()> {
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt().with_env_filter("debug").finish(),
    )?;

    let price_ticker_ws_url = Url::parse("wss://ws.kraken.com")?;
    let mut ticker =
        swap::kraken::connect(price_ticker_ws_url).context("Failed to connect to kraken")?;

    loop {
        match ticker.wait_for_next_update().await? {
            Ok(update) => println!("Price update: {}", update.ask),
            Err(e) => println!("Error: {:#}", e),
        }
    }
}
