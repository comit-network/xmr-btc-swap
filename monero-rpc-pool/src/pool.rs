use anyhow::{Context, Result};
use rand::prelude::*;
use tokio::sync::broadcast;
use tracing::debug;
use typeshare::typeshare;

use crate::database::Database;

#[derive(Debug, Clone, serde::Serialize)]
#[typeshare]
pub struct PoolStatus {
    pub total_node_count: u32,
    pub healthy_node_count: u32,
    #[typeshare(serialized_as = "number")]
    pub successful_health_checks: u64,
    #[typeshare(serialized_as = "number")]
    pub unsuccessful_health_checks: u64,
    pub top_reliable_nodes: Vec<ReliableNodeInfo>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[typeshare]
pub struct ReliableNodeInfo {
    pub url: String,
    pub success_rate: f64,
    pub avg_latency_ms: Option<f64>,
}

pub struct NodePool {
    db: Database,
    network: String,
    status_sender: broadcast::Sender<PoolStatus>,
}

impl NodePool {
    pub fn new(db: Database, network: String) -> (Self, broadcast::Receiver<PoolStatus>) {
        let (status_sender, status_receiver) = broadcast::channel(100);
        let pool = Self {
            db,
            network,
            status_sender,
        };
        (pool, status_receiver)
    }

    /// Get next node using Power of Two Choices algorithm
    /// Only considers identified nodes (nodes with network set)
    pub async fn get_next_node(&self) -> Result<Option<String>> {
        let candidate_nodes = self.db.get_identified_nodes(&self.network).await?;

        if candidate_nodes.is_empty() {
            debug!("No identified nodes available for network {}", self.network);
            return Ok(None);
        }

        if candidate_nodes.len() == 1 {
            return Ok(Some(candidate_nodes[0].full_url()));
        }

        // Power of Two Choices: pick 2 random nodes, select the better one
        let mut rng = thread_rng();
        let node1 = candidate_nodes.choose(&mut rng).unwrap();
        let node2 = candidate_nodes.choose(&mut rng).unwrap();

        let selected =
            if self.calculate_goodness_score(node1) >= self.calculate_goodness_score(node2) {
                node1
            } else {
                node2
            };

        debug!(
            "Selected node using P2C for network {}: {}",
            self.network,
            selected.full_url()
        );

        Ok(Some(selected.full_url()))
    }

    /// Calculate goodness score based on usage-based recency
    /// Score is a function of success rate and latency from last N health checks
    fn calculate_goodness_score(&self, node: &crate::database::MoneroNode) -> f64 {
        let total_checks = node.success_count + node.failure_count;
        if total_checks == 0 {
            return 0.0;
        }

        let success_rate = node.success_count as f64 / total_checks as f64;

        // Weight by recency (more recent interactions = higher weight)
        let recency_weight = (total_checks as f64).min(200.0) / 200.0;
        let mut score = success_rate * recency_weight;

        // Factor in latency - lower latency = higher score
        if let Some(avg_latency) = node.avg_latency_ms {
            let latency_factor = 1.0 - (avg_latency.min(2000.0) / 2000.0);
            score = score * 0.8 + latency_factor * 0.2; // 80% success rate, 20% latency
        }

        score
    }

    pub async fn record_success(
        &self,
        scheme: &str,
        host: &str,
        port: i64,
        latency_ms: f64,
    ) -> Result<()> {
        self.db
            .record_health_check(scheme, host, port, true, Some(latency_ms))
            .await?;
        Ok(())
    }

    pub async fn record_failure(&self, scheme: &str, host: &str, port: i64) -> Result<()> {
        self.db
            .record_health_check(scheme, host, port, false, None)
            .await?;
        Ok(())
    }

    pub async fn publish_status_update(&self) -> Result<()> {
        let status = self.get_current_status().await?;
        let _ = self.status_sender.send(status); // Ignore if no receivers
        Ok(())
    }

    pub async fn get_current_status(&self) -> Result<PoolStatus> {
        let (total, reachable, _reliable) = self.db.get_node_stats(&self.network).await?;
        let reliable_nodes = self.db.get_reliable_nodes(&self.network).await?;
        let (successful_checks, unsuccessful_checks) =
            self.db.get_health_check_stats(&self.network).await?;

        let top_reliable_nodes = reliable_nodes
            .into_iter()
            .take(5)
            .map(|node| ReliableNodeInfo {
                url: node.full_url(),
                success_rate: node.success_rate(),
                avg_latency_ms: node.avg_latency_ms,
            })
            .collect();

        Ok(PoolStatus {
            total_node_count: total as u32,
            healthy_node_count: reachable as u32,
            successful_health_checks: successful_checks,
            unsuccessful_health_checks: unsuccessful_checks,
            top_reliable_nodes,
        })
    }

    /// Get top reliable nodes with fill-up logic to ensure pool size
    /// First tries to get top nodes based on recent success, then fills up with random nodes
    pub async fn get_top_reliable_nodes(
        &self,
        limit: usize,
    ) -> Result<Vec<crate::database::MoneroNode>> {
        debug!(
            "Getting top reliable nodes for network {} (target: {})",
            self.network, limit
        );

        // Step 1: Try primary fetch - get top nodes based on recent success (last 200 health checks)
        let mut top_nodes = self
            .db
            .get_top_nodes_by_recent_success(&self.network, 200, limit as i64)
            .await
            .context("Failed to get top nodes by recent success")?;

        debug!(
            "Primary fetch returned {} nodes for network {} (target: {})",
            top_nodes.len(),
            self.network,
            limit
        );

        // Step 2: If primary fetch didn't return enough nodes, fall back to any identified nodes with successful health checks
        if top_nodes.len() < limit {
            debug!("Primary fetch returned insufficient nodes, falling back to any identified nodes with successful health checks");
            top_nodes = self
                .db
                .get_identified_nodes_with_success(&self.network)
                .await?;

            debug!(
                "Fallback fetch returned {} nodes with successful health checks for network {}",
                top_nodes.len(),
                self.network
            );
        }

        // Step 3: Check if we still don't have enough nodes
        if top_nodes.len() < limit {
            let needed = limit - top_nodes.len();
            debug!(
                "Pool needs {} more nodes to reach target of {} for network {}",
                needed, limit, self.network
            );

            // Step 4: Collect exclusion IDs from nodes already selected
            let exclude_ids: Vec<i64> = top_nodes.iter().filter_map(|node| node.id).collect();

            // Step 5: Secondary fetch - get random nodes to fill up
            let random_fillers = self
                .db
                .get_random_nodes(&self.network, needed as i64, &exclude_ids)
                .await?;

            debug!(
                "Secondary fetch returned {} random nodes for network {}",
                random_fillers.len(),
                self.network
            );

            // Step 6: Combine lists
            top_nodes.extend(random_fillers);
        }

        debug!(
            "Final pool size: {} nodes for network {} (target: {})",
            top_nodes.len(),
            self.network,
            limit
        );

        Ok(top_nodes)
    }

    pub async fn get_pool_stats(&self) -> Result<PoolStats> {
        let (total, reachable, reliable) = self.db.get_node_stats(&self.network).await?;
        let reliable_nodes = self.db.get_reliable_nodes(&self.network).await?;

        let avg_reliable_latency = if reliable_nodes.is_empty() {
            None
        } else {
            let total_latency: f64 = reliable_nodes
                .iter()
                .filter_map(|node| node.avg_latency_ms)
                .sum();
            let count = reliable_nodes
                .iter()
                .filter(|node| node.avg_latency_ms.is_some())
                .count();

            if count > 0 {
                Some(total_latency / count as f64)
            } else {
                None
            }
        };

        Ok(PoolStats {
            total_nodes: total,
            reachable_nodes: reachable,
            reliable_nodes: reliable,
            avg_reliable_latency_ms: avg_reliable_latency,
        })
    }
}

#[derive(Debug)]
pub struct PoolStats {
    pub total_nodes: i64,
    pub reachable_nodes: i64,
    pub reliable_nodes: i64,
    pub avg_reliable_latency_ms: Option<f64>, // TOOD: Why is this an Option, we hate Options
}

impl PoolStats {
    pub fn health_percentage(&self) -> f64 {
        if self.total_nodes == 0 {
            0.0
        } else {
            (self.reachable_nodes as f64 / self.total_nodes as f64) * 100.0
        }
    }
}
