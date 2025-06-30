use std::path::PathBuf;

use crate::types::{NodeAddress, NodeHealthStats, NodeMetadata, NodeRecord};
use anyhow::Result;
use sqlx::SqlitePool;
use tracing::{info, warn};

#[derive(Clone)]
pub struct Database {
    pub pool: SqlitePool,
}

impl Database {
    pub async fn new(data_dir: PathBuf) -> Result<Self> {
        if !data_dir.exists() {
            std::fs::create_dir_all(&data_dir)?;
            info!("Created application data directory: {}", data_dir.display());
        }

        let db_path = data_dir.join("nodes.db");

        info!("Using database at {}", db_path.display());

        let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
        let pool = SqlitePool::connect(&database_url).await?;

        let db = Self { pool };
        db.migrate().await?;

        Ok(db)
    }

    async fn migrate(&self) -> Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;

        info!("Database migration completed");

        Ok(())
    }

    /// Record a health check event
    pub async fn record_health_check(
        &self,
        scheme: &str,
        host: &str,
        port: i64,
        was_successful: bool,
        latency_ms: Option<f64>,
    ) -> Result<()> {
        let result = sqlx::query!(
            r#"
            INSERT INTO health_checks (node_id, timestamp, was_successful, latency_ms)
            SELECT id, datetime('now'), ?, ?
            FROM monero_nodes 
            WHERE scheme = ? AND host = ? AND port = ?
            "#,
            was_successful,
            latency_ms,
            scheme,
            host,
            port
        )
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            warn!(
                "Cannot record health check for unknown node: {}://{}:{}",
                scheme, host, port
            );
        }

        Ok(())
    }

    /// Get reliable nodes (top 4 by reliability score)
    pub async fn get_reliable_nodes(&self, network: &str) -> Result<Vec<NodeRecord>> {
        let rows = sqlx::query!(
            r#"
            SELECT 
                n.id as "id!: i64",
                n.scheme,
                n.host,
                n.port,
                n.network,
                n.first_seen_at,
                CAST(COALESCE(stats.success_count, 0) AS INTEGER) as "success_count!: i64",
                CAST(COALESCE(stats.failure_count, 0) AS INTEGER) as "failure_count!: i64",
                stats.last_success as "last_success?: String",
                stats.last_failure as "last_failure?: String",
                stats.last_checked as "last_checked?: String",
                CAST(1 AS INTEGER) as "is_reliable!: i64",
                stats.avg_latency_ms as "avg_latency_ms?: f64",
                stats.min_latency_ms as "min_latency_ms?: f64",
                stats.max_latency_ms as "max_latency_ms?: f64",
                stats.last_latency_ms as "last_latency_ms?: f64"
            FROM monero_nodes n
            LEFT JOIN (
                SELECT 
                    node_id,
                    SUM(CASE WHEN was_successful THEN 1 ELSE 0 END) as success_count,
                    SUM(CASE WHEN NOT was_successful THEN 1 ELSE 0 END) as failure_count,
                    MAX(CASE WHEN was_successful THEN timestamp END) as last_success,
                    MAX(CASE WHEN NOT was_successful THEN timestamp END) as last_failure,
                    MAX(timestamp) as last_checked,
                    AVG(CASE WHEN was_successful AND latency_ms IS NOT NULL THEN latency_ms END) as avg_latency_ms,
                    MIN(CASE WHEN was_successful AND latency_ms IS NOT NULL THEN latency_ms END) as min_latency_ms,
                    MAX(CASE WHEN was_successful AND latency_ms IS NOT NULL THEN latency_ms END) as max_latency_ms,
                    (SELECT latency_ms FROM health_checks hc2 WHERE hc2.node_id = health_checks.node_id ORDER BY timestamp DESC LIMIT 1) as last_latency_ms
                FROM health_checks 
                GROUP BY node_id
            ) stats ON n.id = stats.node_id
            WHERE n.network = ? AND (COALESCE(stats.success_count, 0) + COALESCE(stats.failure_count, 0)) > 0
            ORDER BY 
                (CAST(COALESCE(stats.success_count, 0) AS REAL) / CAST(COALESCE(stats.success_count, 0) + COALESCE(stats.failure_count, 0) AS REAL)) * 
                (MIN(COALESCE(stats.success_count, 0) + COALESCE(stats.failure_count, 0), 200) / 200.0) * 0.8 +
                CASE 
                    WHEN stats.avg_latency_ms IS NOT NULL THEN (1.0 - (MIN(stats.avg_latency_ms, 2000) / 2000.0)) * 0.2
                    ELSE 0.0 
                END DESC
            LIMIT 4
            "#,
            network
        )
        .fetch_all(&self.pool)
        .await?;

        let nodes: Vec<NodeRecord> = rows
            .into_iter()
            .map(|row| {
                let address = NodeAddress::new(row.scheme, row.host, row.port as u16);
                let first_seen_at = row
                    .first_seen_at
                    .parse()
                    .unwrap_or_else(|_| chrono::Utc::now());

                let metadata = NodeMetadata::new(row.id, row.network, first_seen_at);
                let health = NodeHealthStats {
                    success_count: row.success_count,
                    failure_count: row.failure_count,
                    last_success: row.last_success.and_then(|s| s.parse().ok()),
                    last_failure: row.last_failure.and_then(|s| s.parse().ok()),
                    last_checked: row.last_checked.and_then(|s| s.parse().ok()),
                    avg_latency_ms: row.avg_latency_ms,
                    min_latency_ms: row.min_latency_ms,
                    max_latency_ms: row.max_latency_ms,
                    last_latency_ms: row.last_latency_ms,
                };
                NodeRecord::new(address, metadata, health)
            })
            .collect();

        Ok(nodes)
    }

    /// Get node statistics for a network
    pub async fn get_node_stats(&self, network: &str) -> Result<(i64, i64, i64)> {
        let row = sqlx::query!(
            r#"
            SELECT 
                COUNT(*) as total,
                CAST(SUM(CASE WHEN stats.success_count > 0 THEN 1 ELSE 0 END) AS INTEGER) as "reachable!: i64",
                CAST(SUM(CASE WHEN stats.success_count > stats.failure_count AND stats.success_count > 0 THEN 1 ELSE 0 END) AS INTEGER) as "reliable!: i64"
            FROM monero_nodes n
            LEFT JOIN (
                SELECT 
                    node_id,
                    SUM(CASE WHEN was_successful THEN 1 ELSE 0 END) as success_count,
                    SUM(CASE WHEN NOT was_successful THEN 1 ELSE 0 END) as failure_count
                FROM health_checks 
                GROUP BY node_id
            ) stats ON n.id = stats.node_id
            WHERE n.network = ?
            "#,
            network
        )
        .fetch_one(&self.pool)
        .await?;

        Ok((row.total, row.reachable, row.reliable))
    }

    /// Get health check statistics for a network
    pub async fn get_health_check_stats(&self, network: &str) -> Result<(u64, u64)> {
        let row = sqlx::query!(
            r#"
            SELECT 
                CAST(SUM(CASE WHEN hc.was_successful THEN 1 ELSE 0 END) AS INTEGER) as "successful!: i64",
                CAST(SUM(CASE WHEN NOT hc.was_successful THEN 1 ELSE 0 END) AS INTEGER) as "unsuccessful!: i64"
            FROM (
                SELECT hc.was_successful
                FROM health_checks hc
                JOIN monero_nodes n ON hc.node_id = n.id
                WHERE n.network = ?
                ORDER BY hc.timestamp DESC
                LIMIT 100
            ) hc
            "#,
            network
        )
        .fetch_one(&self.pool)
        .await?;

        let successful = row.successful as u64;
        let unsuccessful = row.unsuccessful as u64;

        Ok((successful, unsuccessful))
    }

    /// Get top nodes based on success rate
    pub async fn get_top_nodes_by_recent_success(
        &self,
        network: &str,
        limit: i64,
    ) -> Result<Vec<NodeAddress>> {
        let rows = sqlx::query!(
            r#"
            SELECT 
                n.scheme,
                n.host,
                n.port
            FROM monero_nodes n
            LEFT JOIN (
                SELECT 
                    node_id,
                    SUM(CASE WHEN was_successful THEN 1 ELSE 0 END) as success_count,
                    SUM(CASE WHEN NOT was_successful THEN 1 ELSE 0 END) as failure_count
                FROM (
                    SELECT node_id, was_successful
                    FROM health_checks 
                    ORDER BY timestamp DESC 
                    LIMIT 1000
                ) recent_checks
                GROUP BY node_id
            ) stats ON n.id = stats.node_id
            WHERE n.network = ?
            ORDER BY 
                CASE 
                    WHEN (COALESCE(stats.success_count, 0) + COALESCE(stats.failure_count, 0)) > 0 
                    THEN CAST(COALESCE(stats.success_count, 0) AS REAL) / CAST(COALESCE(stats.success_count, 0) + COALESCE(stats.failure_count, 0) AS REAL)
                    ELSE 0.0 
                END DESC
            LIMIT ?
            "#,
            network,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        let addresses: Vec<NodeAddress> = rows
            .into_iter()
            .map(|row| NodeAddress::new(row.scheme, row.host, row.port as u16))
            .collect();

        Ok(addresses)
    }
}
