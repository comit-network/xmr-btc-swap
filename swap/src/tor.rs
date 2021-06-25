use anyhow::{anyhow, bail, Result};

pub const DEFAULT_SOCKS5_PORT: u16 = 9050;
pub const DEFAULT_CONTROL_PORT: u16 = 9051;

/// Check if Tor daemon is running on the given port.
pub async fn is_daemon_running_on_port(port: u16) -> Result<()> {
    // Make sure you are running tor and this is your socks port
    let proxy = reqwest::Proxy::all(format!("socks5h://127.0.0.1:{}", port).as_str())
        .map_err(|_| anyhow!("tor proxy should be there"))?;
    let client = reqwest::Client::builder().proxy(proxy).build()?;

    let res = client.get("https://check.torproject.org").send().await?;
    let text = res.text().await?;

    if !text.contains("Congratulations. This browser is configured to use Tor.") {
        bail!("Tor is currently not running")
    }

    Ok(())
}
