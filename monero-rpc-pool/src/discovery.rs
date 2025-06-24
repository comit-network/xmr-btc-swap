use std::collections::HashSet;
use std::time::{Duration, Instant};

use anyhow::Result;
use monero::Network;
use rand::seq::SliceRandom;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use tracing::{error, info, warn};
use url;

use crate::database::Database;

#[derive(Debug, Deserialize)]
struct MoneroFailResponse {
    monero: MoneroNodes,
}

#[derive(Debug, Deserialize)]
struct MoneroNodes {
    clear: Vec<String>,
    #[serde(default)]
    web_compatible: Vec<String>,
}

#[derive(Debug)]
pub struct HealthCheckOutcome {
    pub was_successful: bool,
    pub latency: Duration,
    pub discovered_network: Option<Network>,
}

#[derive(Clone)]
pub struct NodeDiscovery {
    client: Client,
    db: Database,
}

fn network_to_string(network: &Network) -> String {
    match network {
        Network::Mainnet => "mainnet".to_string(),
        Network::Stagenet => "stagenet".to_string(),
        Network::Testnet => "testnet".to_string(),
    }
}

impl NodeDiscovery {
    pub fn new(db: Database) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("monero-rpc-pool/1.0")
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build HTTP client: {}", e))?;

        Ok(Self { client, db })
    }

    /// Fetch nodes from monero.fail API
    pub async fn fetch_mainnet_nodes_from_api(&self) -> Result<Vec<String>> {
        let url = "https://monero.fail/nodes.json?chain=monero";

        let response = self
            .client
            .get(url)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("HTTP error: {}", response.status()));
        }

        let monero_fail_response: MoneroFailResponse = response.json().await?;

        // Combine clear and web_compatible nodes
        let mut nodes = monero_fail_response.monero.web_compatible;
        nodes.extend(monero_fail_response.monero.clear);

        // Remove duplicates using HashSet for O(n) complexity
        let mut seen = HashSet::new();
        let mut unique_nodes = Vec::new();
        for node in nodes {
            if seen.insert(node.clone()) {
                unique_nodes.push(node);
            }
        }

        // Shuffle nodes in random order
        let mut rng = rand::thread_rng();
        unique_nodes.shuffle(&mut rng);

        info!(
            "Fetched {} mainnet nodes from monero.fail API",
            unique_nodes.len()
        );
        Ok(unique_nodes)
    }

    /// Fetch nodes from monero.fail API and discover from other sources
    pub async fn discover_nodes_from_sources(&self, target_network: Network) -> Result<()> {
        // Only fetch from external sources for mainnet to avoid polluting test networks
        if target_network == Network::Mainnet {
            match self.fetch_mainnet_nodes_from_api().await {
                Ok(nodes) => {
                    self.discover_and_insert_nodes(target_network, nodes)
                        .await?;
                }
                Err(e) => {
                    warn!("Failed to fetch nodes from monero.fail API: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Enhanced health check that detects network and validates node identity
    pub async fn check_node_health(
        &self,
        scheme: &str,
        host: &str,
        port: i64,
    ) -> Result<HealthCheckOutcome> {
        let start_time = Instant::now();

        let rpc_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "0",
            "method": "get_info"
        });

        let node_url = format!("{}://{}:{}/json_rpc", scheme, host, port);
        let response = self.client.post(&node_url).json(&rpc_request).send().await;

        let latency = start_time.elapsed();

        match response {
            Ok(resp) => {
                if resp.status().is_success() {
                    match resp.json::<Value>().await {
                        Ok(json) => {
                            if let Some(result) = json.get("result") {
                                // Extract network information from get_info response
                                let discovered_network = self.extract_network_from_info(result);

                                Ok(HealthCheckOutcome {
                                    was_successful: true,
                                    latency,
                                    discovered_network,
                                })
                            } else {
                                Ok(HealthCheckOutcome {
                                    was_successful: false,
                                    latency,
                                    discovered_network: None,
                                })
                            }
                        }
                        Err(_e) => Ok(HealthCheckOutcome {
                            was_successful: false,
                            latency,
                            discovered_network: None,
                        }),
                    }
                } else {
                    Ok(HealthCheckOutcome {
                        was_successful: false,
                        latency,
                        discovered_network: None,
                    })
                }
            }
            Err(_e) => Ok(HealthCheckOutcome {
                was_successful: false,
                latency,
                discovered_network: None,
            }),
        }
    }

    /// Extract network type from get_info response
    fn extract_network_from_info(&self, info_result: &Value) -> Option<Network> {
        // Check nettype field (0 = mainnet, 1 = testnet, 2 = stagenet)
        if let Some(nettype) = info_result.get("nettype").and_then(|v| v.as_u64()) {
            return match nettype {
                0 => Some(Network::Mainnet),
                1 => Some(Network::Testnet),
                2 => Some(Network::Stagenet),
                _ => None,
            };
        }

        // Fallback: check if testnet or stagenet is mentioned in fields
        if let Some(testnet) = info_result.get("testnet").and_then(|v| v.as_bool()) {
            return if testnet {
                Some(Network::Testnet)
            } else {
                Some(Network::Mainnet)
            };
        }

        // Additional heuristics could be added here
        None
    }

    /// Updated health check workflow with identification and validation logic
    pub async fn health_check_all_nodes(&self, target_network: Network) -> Result<()> {
        info!(
            "Starting health check for all nodes targeting network: {}",
            network_to_string(&target_network)
        );

        // Get all nodes from database with proper field mapping
        let all_nodes = sqlx::query!(
            r#"
            SELECT 
                id as "id!: i64",
                scheme,
                host,
                port,
                network as "network!: String",
                first_seen_at
            FROM monero_nodes 
            ORDER BY id
            "#
        )
        .fetch_all(&self.db.pool)
        .await?;

        let mut checked_count = 0;
        let mut healthy_count = 0;
        let mut corrected_count = 0;

        for node in all_nodes {
            match self
                .check_node_health(&node.scheme, &node.host, node.port)
                .await
            {
                Ok(outcome) => {
                    // Always record the health check
                    self.db
                        .record_health_check(
                            &node.scheme,
                            &node.host,
                            node.port,
                            outcome.was_successful,
                            if outcome.was_successful {
                                Some(outcome.latency.as_millis() as f64)
                            } else {
                                None
                            },
                        )
                        .await?;

                    if outcome.was_successful {
                        healthy_count += 1;

                        // Validate network consistency
                        if let Some(discovered_network) = outcome.discovered_network {
                            let discovered_network_str = network_to_string(&discovered_network);
                            if node.network != discovered_network_str {
                                let node_url =
                                    format!("{}://{}:{}", node.scheme, node.host, node.port);
                                warn!("Network mismatch detected for node {}: stored={}, discovered={}. Correcting...", 
                                      node_url, node.network, discovered_network_str);
                                self.db
                                    .update_node_network(
                                        &node.scheme,
                                        &node.host,
                                        node.port,
                                        &discovered_network_str,
                                    )
                                    .await?;
                                corrected_count += 1;
                            }
                        }
                    }
                    checked_count += 1;
                }
                Err(_e) => {
                    self.db
                        .record_health_check(&node.scheme, &node.host, node.port, false, None)
                        .await?;
                }
            }

            // Small delay to avoid hammering nodes
            tokio::time::sleep(Duration::from_secs(2)).await;
        }

        info!(
            "Health check completed: {}/{} nodes healthy, {} corrected",
            healthy_count, checked_count, corrected_count
        );

        Ok(())
    }

    /// Periodic discovery task with improved error handling
    pub async fn periodic_discovery_task(&self, target_network: Network) -> Result<()> {
        let mut interval = tokio::time::interval(Duration::from_secs(3600)); // Every hour

        loop {
            interval.tick().await;

            info!(
                "Running periodic node discovery for network: {}",
                network_to_string(&target_network)
            );

            // Discover new nodes from sources
            if let Err(e) = self.discover_nodes_from_sources(target_network).await {
                error!("Failed to discover nodes: {}", e);
            }

            // Health check all nodes (will identify networks automatically)
            if let Err(e) = self.health_check_all_nodes(target_network).await {
                error!("Failed to perform health check: {}", e);
            }

            // Log stats for all networks
            for network in &[Network::Mainnet, Network::Stagenet, Network::Testnet] {
                let network_str = network_to_string(network);
                if let Ok((total, reachable, reliable)) = self.db.get_node_stats(&network_str).await
                {
                    if total > 0 {
                        info!(
                            "Node stats for {}: {} total, {} reachable, {} reliable",
                            network_str, total, reachable, reliable
                        );
                    }
                }
            }
        }
    }

    /// Insert configured nodes for a specific network
    pub async fn discover_and_insert_nodes(
        &self,
        target_network: Network,
        nodes: Vec<String>,
    ) -> Result<()> {
        let mut success_count = 0;
        let mut error_count = 0;
        let target_network_str = network_to_string(&target_network);

        for node_url in nodes.iter() {
            if let Ok(url) = url::Url::parse(node_url) {
                let scheme = url.scheme();

                // Validate scheme - must be http or https
                if !matches!(scheme, "http" | "https") {
                    continue;
                }

                // Validate host - must be non-empty
                let Some(host) = url.host_str() else {
                    continue;
                };
                if host.is_empty() {
                    continue;
                }

                // Validate port - must be present
                let Some(port) = url.port() else {
                    continue;
                };
                let port = port as i64;

                match self
                    .db
                    .upsert_node(scheme, host, port, &target_network_str)
                    .await
                {
                    Ok(_) => {
                        success_count += 1;
                    }
                    Err(e) => {
                        error_count += 1;
                        error!(
                            "Failed to insert configured node {}://{}:{}: {}",
                            scheme, host, port, e
                        );
                    }
                }
            } else {
                error_count += 1;
                error!("Failed to parse node URL: {}", node_url);
            }
        }

        info!(
            "Configured node insertion complete: {} successful, {} errors",
            success_count, error_count
        );
        Ok(())
    }
}
