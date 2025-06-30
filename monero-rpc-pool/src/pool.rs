use anyhow::{Context, Result};
use tokio::sync::broadcast;
use tracing::{debug, warn};
use typeshare::typeshare;

use crate::database::Database;
use crate::types::NodeAddress;

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

        if let Err(e) = self.status_sender.send(status.clone()) {
            warn!("Failed to send status update: {}", e);
        } else {
            debug!(?status, "Sent status update");
        }

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
                avg_latency_ms: node.health.avg_latency_ms,
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

    /// Get nodes to use, with weighted selection favoring top performers
    /// The list has some randomness, but the top nodes are still more likely to be chosen
    pub async fn get_top_reliable_nodes(&self, limit: usize) -> Result<Vec<NodeAddress>> {
        use rand::seq::SliceRandom;

        debug!(
            "Getting top reliable nodes for network {} (target: {})",
            self.network, limit
        );

        let available_nodes = self
            .db
            .get_top_nodes_by_recent_success(&self.network, limit as i64)
            .await
            .context("Failed to get top nodes by recent success")?;

        let total_candidates = available_nodes.len();

        let weighted: Vec<(NodeAddress, f64)> = available_nodes
            .into_iter()
            .enumerate()
            .map(|(idx, node)| {
                // Higher-ranked (smaller idx) ⇒ larger weight
                let weight = 1.5_f64.powi((total_candidates - idx) as i32);
                (node, weight)
            })
            .collect();

        let mut rng = rand::thread_rng();

        let mut candidates = weighted;
        let mut selected_nodes = Vec::with_capacity(limit);

        while selected_nodes.len() < limit && !candidates.is_empty() {
            // Choose one node based on its weight using `choose_weighted`
            let chosen_pair = candidates
                .choose_weighted(&mut rng, |item| item.1)
                .map_err(|e| anyhow::anyhow!("Weighted choice failed: {}", e))?;

            // Locate index of the chosen pair and remove it
            let chosen_index = candidates
                .iter()
                .position(|x| std::ptr::eq(x, chosen_pair))
                .expect("Chosen item must exist in candidates");

            let (node, _) = candidates.swap_remove(chosen_index);
            selected_nodes.push(node);
        }

        debug!(
            "Pool size: {} nodes for network {} (target: {})",
            selected_nodes.len(),
            self.network,
            limit
        );

        Ok(selected_nodes)
    }

    pub async fn get_pool_stats(&self) -> Result<PoolStats> {
        let (total, reachable, reliable) = self.db.get_node_stats(&self.network).await?;
        let reliable_nodes = self.db.get_reliable_nodes(&self.network).await?;

        let avg_reliable_latency = if reliable_nodes.is_empty() {
            None
        } else {
            let total_latency: f64 = reliable_nodes
                .iter()
                .filter_map(|node| node.health.avg_latency_ms)
                .sum();
            let count = reliable_nodes
                .iter()
                .filter(|node| node.health.avg_latency_ms.is_some())
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
