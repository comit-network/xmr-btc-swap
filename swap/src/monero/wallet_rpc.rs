use ::monero::Network;
use anyhow::{bail, Context, Error, Result};
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::time::Duration;

// See: https://www.moneroworld.com/#nodes, https://monero.fail
// We don't need any testnet nodes because we don't support testnet at all
static MONERO_DAEMONS: Lazy<[MoneroDaemon; 12]> = Lazy::new(|| {
    [
        MoneroDaemon::new("http://xmr-node.cakewallet.com:18081", Network::Mainnet),
        MoneroDaemon::new("http://nodex.monerujo.io:18081", Network::Mainnet),
        MoneroDaemon::new("http://nodes.hashvault.pro:18081", Network::Mainnet),
        MoneroDaemon::new("http://p2pmd.xmrvsbeast.com:18081", Network::Mainnet),
        MoneroDaemon::new("http://node.monerodevs.org:18089", Network::Mainnet),
        MoneroDaemon::new("http://xmr-node-uk.cakewallet.com:18081", Network::Mainnet),
        MoneroDaemon::new("http://xmr.litepay.ch:18081", Network::Mainnet),
        MoneroDaemon::new("http://stagenet.xmr-tw.org:38081", Network::Stagenet),
        MoneroDaemon::new("http://node.monerodevs.org:38089", Network::Stagenet),
        MoneroDaemon::new("http://singapore.node.xmr.pm:38081", Network::Stagenet),
        MoneroDaemon::new("http://xmr-lux.boldsuck.org:38081", Network::Stagenet),
        MoneroDaemon::new("http://stagenet.community.rino.io:38081", Network::Stagenet),
    ]
});

#[derive(Debug, Clone)]
pub struct MoneroDaemon {
    url: String,
    network: Network,
}

impl MoneroDaemon {
    pub fn new(url: impl Into<String>, network: Network) -> MoneroDaemon {
        MoneroDaemon {
            url: url.into(),
            network,
        }
    }

    pub fn from_str(url: impl Into<String>, network: Network) -> Result<MoneroDaemon, Error> {
        Ok(MoneroDaemon {
            url: url.into(),
            network,
        })
    }

    /// Checks if the Monero daemon is available by sending a request to its `get_info` endpoint.
    pub async fn is_available(&self, client: &reqwest::Client) -> Result<bool, Error> {
        let url = if self.url.ends_with("/") {
            format!("{}get_info", self.url)
        } else {
            format!("{}/get_info", self.url)
        };

        let res = client
            .get(&url)
            .send()
            .await
            .context("Failed to send request to get_info endpoint")?;

        let json: MoneroDaemonGetInfoResponse = res
            .json()
            .await
            .context("Failed to deserialize daemon get_info response")?;

        let is_status_ok = json.status == "OK";
        let is_synchronized = json.synchronized;
        let is_correct_network = match self.network {
            Network::Mainnet => json.mainnet,
            Network::Stagenet => json.stagenet,
            Network::Testnet => json.testnet,
        };

        Ok(is_status_ok && is_synchronized && is_correct_network)
    }
}

impl Display for MoneroDaemon {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.url)
    }
}

#[derive(Deserialize)]
struct MoneroDaemonGetInfoResponse {
    status: String,
    synchronized: bool,
    mainnet: bool,
    stagenet: bool,
    testnet: bool,
}

/// Chooses an available Monero daemon based on the specified network.
async fn choose_monero_daemon(network: Network) -> Result<MoneroDaemon, Error> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .https_only(false)
        .build()?;

    // We only want to check for daemons that match the specified network
    let network_matching_daemons = MONERO_DAEMONS
        .iter()
        .filter(|daemon| daemon.network == network);

    for daemon in network_matching_daemons {
        match daemon.is_available(&client).await {
            Ok(true) => {
                tracing::debug!(%daemon, "Found available Monero daemon");
                return Ok(daemon.clone());
            }
            Err(err) => {
                tracing::debug!(?err, %daemon, "Failed to connect to Monero daemon");
                continue;
            }
            Ok(false) => continue,
        }
    }

    bail!("No Monero daemon could be found. Please specify one manually or try again later.")
}

/// Public wrapper around [`choose_monero_daemon`].
pub async fn choose_monero_node(network: Network) -> Result<MoneroDaemon, Error> {
    choose_monero_daemon(network).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_is_daemon_available_success() {
        let mut server = mockito::Server::new_async().await;

        let _ = server
            .mock("GET", "/get_info")
            .with_status(200)
            .with_body(
                r#"
                {
                    "status": "OK",
                    "synchronized": true,
                    "mainnet": true,
                    "stagenet": false,
                    "testnet": false
                }
                "#,
            )
            .create();

        let url = format!("http://{}", server.url());

        let client = reqwest::Client::new();
        let result = MoneroDaemon::new(url, Network::Mainnet)
            .is_available(&client)
            .await;

        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn test_is_daemon_available_wrong_network_failure() {
        let mut server = mockito::Server::new_async().await;

        let _ = server
            .mock("GET", "/get_info")
            .with_status(200)
            .with_body(
                r#"
                {
                    "status": "OK",
                    "synchronized": true,
                    "mainnet": true,
                    "stagenet": false,
                    "testnet": false
                }
                "#,
            )
            .create();

        let url = format!("http://{}", server.url());

        let client = reqwest::Client::new();
        let result = MoneroDaemon::new(url, Network::Stagenet)
            .is_available(&client)
            .await;

        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn test_is_daemon_available_not_synced_failure() {
        let mut server = mockito::Server::new_async().await;

        let _ = server
            .mock("GET", "/get_info")
            .with_status(200)
            .with_body(
                r#"
                {
                    "status": "OK",
                    "synchronized": false,
                    "mainnet": true,
                    "stagenet": false,
                    "testnet": false
                }
                "#,
            )
            .create();

        let url = format!("http://{}", server.url());

        let client = reqwest::Client::new();
        let result = MoneroDaemon::new(url, Network::Mainnet)
            .is_available(&client)
            .await;

        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn test_is_daemon_available_network_error_failure() {
        let client = reqwest::Client::new();
        let result = MoneroDaemon::new("http://does.not.exist.com:18081", Network::Mainnet)
            .is_available(&client)
            .await;

        assert!(result.is_err());
    }
}
