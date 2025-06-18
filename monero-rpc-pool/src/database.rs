use std::path::PathBuf;

use anyhow::Result;
use dirs::data_dir;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MoneroNode {
    pub id: Option<i64>,
    pub scheme: String, // http or https
    pub host: String,
    pub port: i64,
    pub full_url: String,
    pub network: String, // mainnet, stagenet, or testnet - always known at insertion time
    pub first_seen_at: String, // ISO 8601 timestamp when first discovered
    // Computed fields from health_checks (not stored in monero_nodes table)
    #[sqlx(default)]
    pub success_count: i64,
    #[sqlx(default)]
    pub failure_count: i64,
    #[sqlx(default)]
    pub last_success: Option<String>,
    #[sqlx(default)]
    pub last_failure: Option<String>,
    #[sqlx(default)]
    pub last_checked: Option<String>,
    #[sqlx(default)]
    pub is_reliable: bool,
    #[sqlx(default)]
    pub avg_latency_ms: Option<f64>,
    #[sqlx(default)]
    pub min_latency_ms: Option<f64>,
    #[sqlx(default)]
    pub max_latency_ms: Option<f64>,
    #[sqlx(default)]
    pub last_latency_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct HealthCheck {
    pub id: Option<i64>,
    pub node_id: i64,
    pub timestamp: String, // ISO 8601 timestamp
    pub was_successful: bool,
    pub latency_ms: Option<f64>,
}

impl MoneroNode {
    pub fn new(scheme: String, host: String, port: i64, network: String) -> Self {
        let full_url = format!("{}://{}:{}", scheme, host, port);
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            id: None,
            scheme,
            host,
            port,
            full_url,
            network,
            first_seen_at: now,
            // These are computed from health_checks
            success_count: 0,
            failure_count: 0,
            last_success: None,
            last_failure: None,
            last_checked: None,
            is_reliable: false,
            avg_latency_ms: None,
            min_latency_ms: None,
            max_latency_ms: None,
            last_latency_ms: None,
        }
    }

    pub fn success_rate(&self) -> f64 {
        let total = self.success_count + self.failure_count;
        if total == 0 {
            0.0
        } else {
            self.success_count as f64 / total as f64
        }
    }

    pub fn reliability_score(&self) -> f64 {
        let success_rate = self.success_rate();
        let total_requests = self.success_count + self.failure_count;

        // Weight success rate by total requests (more requests = more reliable data)
        let request_weight = (total_requests as f64).min(200.0) / 200.0;
        let mut score = success_rate * request_weight;

        // Factor in latency - lower latency = higher score
        if let Some(avg_latency) = self.avg_latency_ms {
            // Normalize latency to 0-1 range (assuming 0-2000ms range)
            let latency_factor = 1.0 - (avg_latency.min(2000.0) / 2000.0);
            score = score * 0.8 + latency_factor * 0.2; // 80% success rate, 20% latency
        }

        score
    }
}

#[derive(Clone)]
pub struct Database {
    pub pool: SqlitePool,
}

impl Database {
    pub async fn new() -> Result<Self> {
        let app_data_dir = get_app_data_dir()?;
        Self::new_with_data_dir(app_data_dir).await
    }

    pub async fn new_with_data_dir(data_dir: PathBuf) -> Result<Self> {
        if !data_dir.exists() {
            std::fs::create_dir_all(&data_dir)?;
            info!("Created application data directory: {}", data_dir.display());
        }

        let db_path = data_dir.join("nodes.db");
        info!("Using database at: {}", db_path.display());

        let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
        let pool = SqlitePool::connect(&database_url).await?;

        let db = Self { pool };
        db.migrate().await?;

        Ok(db)
    }

    async fn migrate(&self) -> Result<()> {
        // Run sqlx migrations
        sqlx::migrate!("./migrations").run(&self.pool).await?;

        info!("Database migration completed");
        Ok(())
    }

    /// Insert a node if it doesn't exist, return the node_id
    pub async fn upsert_node(
        &self,
        scheme: &str,
        host: &str,
        port: i64,
        network: &str,
    ) -> Result<i64> {
        let full_url = format!("{}://{}:{}", scheme, host, port);
        let now = chrono::Utc::now().to_rfc3339();

        let result = sqlx::query!(
            r#"
            INSERT INTO monero_nodes (scheme, host, port, full_url, network, first_seen_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(full_url) DO UPDATE SET
                network = excluded.network,
                updated_at = excluded.updated_at
            RETURNING id
            "#,
            scheme,
            host,
            port,
            full_url,
            network,
            now,
            now
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(result.id)
    }

    /// Update a node's network after it has been identified
    pub async fn update_node_network(&self, url: &str, network: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();

        let result = sqlx::query!(
            r#"
            UPDATE monero_nodes 
            SET network = ?, updated_at = ?
            WHERE full_url = ?
            "#,
            network,
            now,
            url
        )
        .execute(&self.pool)
        .await?;

        if result.rows_affected() > 0 {
            debug!("Updated network for node {} to {}", url, network);
        } else {
            warn!("Failed to update network for node {}: not found", url);
        }

        Ok(())
    }

    /// Record a health check event
    pub async fn record_health_check(
        &self,
        url: &str,
        was_successful: bool,
        latency_ms: Option<f64>,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();

        // First get the node_id
        let node_row = sqlx::query!("SELECT id FROM monero_nodes WHERE full_url = ?", url)
            .fetch_optional(&self.pool)
            .await?;

        let node_id = match node_row {
            Some(row) => row.id,
            None => {
                warn!("Cannot record health check for unknown node: {}", url);
                return Ok(());
            }
        };

        sqlx::query!(
            r#"
            INSERT INTO health_checks (node_id, timestamp, was_successful, latency_ms)
            VALUES (?, ?, ?, ?)
            "#,
            node_id,
            now,
            was_successful,
            latency_ms
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get nodes that have been identified (have network set)
    pub async fn get_identified_nodes(&self, network: &str) -> Result<Vec<MoneroNode>> {
        let rows = sqlx::query!(
            r#"
            SELECT 
                n.id as "id!: i64",
                n.scheme,
                n.host,
                n.port,
                n.full_url,
                n.network,
                n.first_seen_at,
                CAST(COALESCE(stats.success_count, 0) AS INTEGER) as "success_count!: i64",
                CAST(COALESCE(stats.failure_count, 0) AS INTEGER) as "failure_count!: i64",
                stats.last_success as "last_success?: String",
                stats.last_failure as "last_failure?: String",
                stats.last_checked as "last_checked?: String",
                CAST(CASE WHEN reliable_nodes.node_id IS NOT NULL THEN 1 ELSE 0 END AS INTEGER) as "is_reliable!: i64",
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
            LEFT JOIN (
                SELECT DISTINCT node_id FROM (
                    SELECT 
                        n2.id as node_id,
                        COALESCE(s2.success_count, 0) as success_count,
                        COALESCE(s2.failure_count, 0) as failure_count,
                        s2.avg_latency_ms,
                        (CAST(COALESCE(s2.success_count, 0) AS REAL) / CAST(COALESCE(s2.success_count, 0) + COALESCE(s2.failure_count, 0) AS REAL)) * 
                        (MIN(COALESCE(s2.success_count, 0) + COALESCE(s2.failure_count, 0), 200) / 200.0) * 0.8 +
                        CASE 
                            WHEN s2.avg_latency_ms IS NOT NULL THEN (1.0 - (MIN(s2.avg_latency_ms, 2000) / 2000.0)) * 0.2
                            ELSE 0.0 
                        END as reliability_score
                    FROM monero_nodes n2
                    LEFT JOIN (
                        SELECT 
                            node_id,
                            SUM(CASE WHEN was_successful THEN 1 ELSE 0 END) as success_count,
                            SUM(CASE WHEN NOT was_successful THEN 1 ELSE 0 END) as failure_count,
                            AVG(CASE WHEN was_successful AND latency_ms IS NOT NULL THEN latency_ms END) as avg_latency_ms
                        FROM health_checks 
                        GROUP BY node_id
                    ) s2 ON n2.id = s2.node_id
                    WHERE n2.network = ? AND (COALESCE(s2.success_count, 0) + COALESCE(s2.failure_count, 0)) > 0
                    ORDER BY reliability_score DESC
                    LIMIT 4
                )
            ) reliable_nodes ON n.id = reliable_nodes.node_id
            WHERE n.network = ?
            ORDER BY stats.avg_latency_ms ASC, stats.success_count DESC
            "#,
            network,
            network
        )
        .fetch_all(&self.pool)
        .await?;

        let nodes: Vec<MoneroNode> = rows
            .into_iter()
            .map(|row| MoneroNode {
                id: Some(row.id),
                scheme: row.scheme,
                host: row.host,
                port: row.port,
                full_url: row.full_url,
                network: row.network,
                first_seen_at: row.first_seen_at,
                success_count: row.success_count,
                failure_count: row.failure_count,
                last_success: row.last_success,
                last_failure: row.last_failure,
                last_checked: row.last_checked,
                is_reliable: row.is_reliable != 0,
                avg_latency_ms: row.avg_latency_ms,
                min_latency_ms: row.min_latency_ms,
                max_latency_ms: row.max_latency_ms,
                last_latency_ms: row.last_latency_ms,
            })
            .collect();

        debug!(
            "Retrieved {} identified nodes for network {}",
            nodes.len(),
            network
        );
        Ok(nodes)
    }

    /// Get reliable nodes (top 4 by reliability score)
    pub async fn get_reliable_nodes(&self, network: &str) -> Result<Vec<MoneroNode>> {
        let rows = sqlx::query!(
            r#"
            SELECT 
                n.id as "id!: i64",
                n.scheme,
                n.host,
                n.port,
                n.full_url,
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

        let nodes = rows
            .into_iter()
            .map(|row| MoneroNode {
                id: Some(row.id),
                scheme: row.scheme,
                host: row.host,
                port: row.port,
                full_url: row.full_url,
                network: row.network,
                first_seen_at: row.first_seen_at,
                success_count: row.success_count,
                failure_count: row.failure_count,
                last_success: row.last_success,
                last_failure: row.last_failure,
                last_checked: row.last_checked,
                is_reliable: true,
                avg_latency_ms: row.avg_latency_ms,
                min_latency_ms: row.min_latency_ms,
                max_latency_ms: row.max_latency_ms,
                last_latency_ms: row.last_latency_ms,
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
                CAST((SELECT COUNT(*) FROM (
                    SELECT n2.id
                    FROM monero_nodes n2
                    LEFT JOIN (
                        SELECT 
                            node_id,
                            SUM(CASE WHEN was_successful THEN 1 ELSE 0 END) as success_count,
                            SUM(CASE WHEN NOT was_successful THEN 1 ELSE 0 END) as failure_count,
                            AVG(CASE WHEN was_successful AND latency_ms IS NOT NULL THEN latency_ms END) as avg_latency_ms
                        FROM health_checks 
                        GROUP BY node_id
                    ) s2 ON n2.id = s2.node_id
                    WHERE n2.network = ? AND (COALESCE(s2.success_count, 0) + COALESCE(s2.failure_count, 0)) > 0
                    ORDER BY 
                        (CAST(COALESCE(s2.success_count, 0) AS REAL) / CAST(COALESCE(s2.success_count, 0) + COALESCE(s2.failure_count, 0) AS REAL)) * 
                        (MIN(COALESCE(s2.success_count, 0) + COALESCE(s2.failure_count, 0), 200) / 200.0) * 0.8 +
                        CASE 
                            WHEN s2.avg_latency_ms IS NOT NULL THEN (1.0 - (MIN(s2.avg_latency_ms, 2000) / 2000.0)) * 0.2
                            ELSE 0.0 
                        END DESC
                    LIMIT 4
                )) AS INTEGER) as "reliable!: i64"
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
            network,
            network
        )
        .fetch_one(&self.pool)
        .await?;

        let total = row.total;
        let reachable = row.reachable;
        let reliable = row.reliable;

        Ok((total, reachable, reliable))
    }

    /// Get health check statistics for a network
    pub async fn get_health_check_stats(&self, network: &str) -> Result<(u64, u64)> {
        let row = sqlx::query!(
            r#"
            SELECT 
                CAST(SUM(CASE WHEN hc.was_successful THEN 1 ELSE 0 END) AS INTEGER) as "successful!: i64",
                CAST(SUM(CASE WHEN NOT hc.was_successful THEN 1 ELSE 0 END) AS INTEGER) as "unsuccessful!: i64"
            FROM health_checks hc
            JOIN monero_nodes n ON hc.node_id = n.id
            WHERE n.network = ?
            "#,
            network
        )
        .fetch_one(&self.pool)
        .await?;

        let successful = row.successful as u64;
        let unsuccessful = row.unsuccessful as u64;

        Ok((successful, unsuccessful))
    }

    /// Get top nodes based on recent success rate and latency
    pub async fn get_top_nodes_by_recent_success(
        &self,
        network: &str,
        _recent_checks_limit: i64,
        limit: i64,
    ) -> Result<Vec<MoneroNode>> {
        let rows = sqlx::query!(
            r#"
            SELECT 
                n.id as "id!: i64",
                n.scheme,
                n.host,
                n.port,
                n.full_url,
                n.network,
                n.first_seen_at,
                CAST(COALESCE(stats.success_count, 0) AS INTEGER) as "success_count!: i64",
                CAST(COALESCE(stats.failure_count, 0) AS INTEGER) as "failure_count!: i64",
                stats.last_success as "last_success?: String",
                stats.last_failure as "last_failure?: String",
                stats.last_checked as "last_checked?: String",
                CAST(CASE WHEN reliable_nodes.node_id IS NOT NULL THEN 1 ELSE 0 END AS INTEGER) as "is_reliable!: i64",
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
            LEFT JOIN (
                SELECT DISTINCT node_id FROM (
                    SELECT 
                        n2.id as node_id,
                        COALESCE(s2.success_count, 0) as success_count,
                        COALESCE(s2.failure_count, 0) as failure_count,
                        s2.avg_latency_ms,
                        (CAST(COALESCE(s2.success_count, 0) AS REAL) / CAST(COALESCE(s2.success_count, 0) + COALESCE(s2.failure_count, 0) AS REAL)) * 
                        (MIN(COALESCE(s2.success_count, 0) + COALESCE(s2.failure_count, 0), 200) / 200.0) * 0.8 +
                        CASE 
                            WHEN s2.avg_latency_ms IS NOT NULL THEN (1.0 - (MIN(s2.avg_latency_ms, 2000) / 2000.0)) * 0.2
                            ELSE 0.0 
                        END as reliability_score
                    FROM monero_nodes n2
                    LEFT JOIN (
                        SELECT 
                            node_id,
                            SUM(CASE WHEN was_successful THEN 1 ELSE 0 END) as success_count,
                            SUM(CASE WHEN NOT was_successful THEN 1 ELSE 0 END) as failure_count,
                            AVG(CASE WHEN was_successful AND latency_ms IS NOT NULL THEN latency_ms END) as avg_latency_ms
                        FROM health_checks 
                        GROUP BY node_id
                    ) s2 ON n2.id = s2.node_id
                    WHERE n2.network = ? AND (COALESCE(s2.success_count, 0) + COALESCE(s2.failure_count, 0)) > 0
                    ORDER BY reliability_score DESC
                    LIMIT 4
                )
            ) reliable_nodes ON n.id = reliable_nodes.node_id
            WHERE n.network = ? AND (COALESCE(stats.success_count, 0) + COALESCE(stats.failure_count, 0)) > 0
            ORDER BY 
                (CAST(COALESCE(stats.success_count, 0) AS REAL) / CAST(COALESCE(stats.success_count, 0) + COALESCE(stats.failure_count, 0) AS REAL)) DESC,
                stats.avg_latency_ms ASC
            LIMIT ?
            "#,
            network,
            network,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        let nodes = rows
            .into_iter()
            .map(|row| MoneroNode {
                id: Some(row.id),
                scheme: row.scheme,
                host: row.host,
                port: row.port,
                full_url: row.full_url,
                network: row.network,
                first_seen_at: row.first_seen_at,
                success_count: row.success_count,
                failure_count: row.failure_count,
                last_success: row.last_success,
                last_failure: row.last_failure,
                last_checked: row.last_checked,
                is_reliable: row.is_reliable != 0,
                avg_latency_ms: row.avg_latency_ms,
                min_latency_ms: row.min_latency_ms,
                max_latency_ms: row.max_latency_ms,
                last_latency_ms: row.last_latency_ms,
            })
            .collect();

        Ok(nodes)
    }

    /// Get identified nodes that have at least one successful health check
    pub async fn get_identified_nodes_with_success(
        &self,
        network: &str,
    ) -> Result<Vec<MoneroNode>> {
        let rows = sqlx::query!(
            r#"
            SELECT 
                n.id as "id!: i64",
                n.scheme,
                n.host,
                n.port,
                n.full_url,
                n.network,
                n.first_seen_at,
                CAST(COALESCE(stats.success_count, 0) AS INTEGER) as "success_count!: i64",
                CAST(COALESCE(stats.failure_count, 0) AS INTEGER) as "failure_count!: i64",
                stats.last_success as "last_success?: String",
                stats.last_failure as "last_failure?: String",
                stats.last_checked as "last_checked?: String",
                CAST(CASE WHEN reliable_nodes.node_id IS NOT NULL THEN 1 ELSE 0 END AS INTEGER) as "is_reliable!: i64",
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
            LEFT JOIN (
                SELECT DISTINCT node_id FROM (
                    SELECT 
                        n2.id as node_id,
                        COALESCE(s2.success_count, 0) as success_count,
                        COALESCE(s2.failure_count, 0) as failure_count,
                        s2.avg_latency_ms,
                        (CAST(COALESCE(s2.success_count, 0) AS REAL) / CAST(COALESCE(s2.success_count, 0) + COALESCE(s2.failure_count, 0) AS REAL)) * 
                        (MIN(COALESCE(s2.success_count, 0) + COALESCE(s2.failure_count, 0), 200) / 200.0) * 0.8 +
                        CASE 
                            WHEN s2.avg_latency_ms IS NOT NULL THEN (1.0 - (MIN(s2.avg_latency_ms, 2000) / 2000.0)) * 0.2
                            ELSE 0.0 
                        END as reliability_score
                    FROM monero_nodes n2
                    LEFT JOIN (
                        SELECT 
                            node_id,
                            SUM(CASE WHEN was_successful THEN 1 ELSE 0 END) as success_count,
                            SUM(CASE WHEN NOT was_successful THEN 1 ELSE 0 END) as failure_count,
                            AVG(CASE WHEN was_successful AND latency_ms IS NOT NULL THEN latency_ms END) as avg_latency_ms
                        FROM health_checks 
                        GROUP BY node_id
                    ) s2 ON n2.id = s2.node_id
                    WHERE n2.network = ? AND (COALESCE(s2.success_count, 0) + COALESCE(s2.failure_count, 0)) > 0
                    ORDER BY reliability_score DESC
                    LIMIT 4
                )
            ) reliable_nodes ON n.id = reliable_nodes.node_id
            WHERE n.network = ? AND stats.success_count > 0
            ORDER BY stats.avg_latency_ms ASC, stats.success_count DESC
            "#,
            network,
            network
        )
        .fetch_all(&self.pool)
        .await?;

        let nodes: Vec<MoneroNode> = rows
            .into_iter()
            .map(|row| MoneroNode {
                id: Some(row.id),
                scheme: row.scheme,
                host: row.host,
                port: row.port,
                full_url: row.full_url,
                network: row.network,
                first_seen_at: row.first_seen_at,
                success_count: row.success_count,
                failure_count: row.failure_count,
                last_success: row.last_success,
                last_failure: row.last_failure,
                last_checked: row.last_checked,
                is_reliable: row.is_reliable != 0,
                avg_latency_ms: row.avg_latency_ms,
                min_latency_ms: row.min_latency_ms,
                max_latency_ms: row.max_latency_ms,
                last_latency_ms: row.last_latency_ms,
            })
            .collect();

        debug!(
            "Retrieved {} identified nodes with success for network {}",
            nodes.len(),
            network
        );
        Ok(nodes)
    }

    /// Get random nodes for the specified network, excluding specific IDs
    pub async fn get_random_nodes(
        &self,
        network: &str,
        limit: i64,
        exclude_ids: &[i64],
    ) -> Result<Vec<MoneroNode>> {
        if exclude_ids.is_empty() {
            let rows = sqlx::query!(
                r#"
                SELECT 
                    n.id as "id!: i64",
                    n.scheme,
                    n.host,
                    n.port,
                    n.full_url,
                    n.network,
                    n.first_seen_at,
                    CAST(COALESCE(stats.success_count, 0) AS INTEGER) as "success_count!: i64",
                    CAST(COALESCE(stats.failure_count, 0) AS INTEGER) as "failure_count!: i64",
                    stats.last_success as "last_success?: String",
                    stats.last_failure as "last_failure?: String",
                    stats.last_checked as "last_checked?: String",
                    CAST(CASE WHEN reliable_nodes.node_id IS NOT NULL THEN 1 ELSE 0 END AS INTEGER) as "is_reliable!: i64",
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
                LEFT JOIN (
                    SELECT DISTINCT node_id FROM (
                        SELECT 
                            n2.id as node_id,
                            COALESCE(s2.success_count, 0) as success_count,
                            COALESCE(s2.failure_count, 0) as failure_count,
                            s2.avg_latency_ms,
                            (CAST(COALESCE(s2.success_count, 0) AS REAL) / CAST(COALESCE(s2.success_count, 0) + COALESCE(s2.failure_count, 0) AS REAL)) * 
                            (MIN(COALESCE(s2.success_count, 0) + COALESCE(s2.failure_count, 0), 200) / 200.0) * 0.8 +
                            CASE 
                                WHEN s2.avg_latency_ms IS NOT NULL THEN (1.0 - (MIN(s2.avg_latency_ms, 2000) / 2000.0)) * 0.2
                                ELSE 0.0 
                            END as reliability_score
                        FROM monero_nodes n2
                        LEFT JOIN (
                            SELECT 
                                node_id,
                                SUM(CASE WHEN was_successful THEN 1 ELSE 0 END) as success_count,
                                SUM(CASE WHEN NOT was_successful THEN 1 ELSE 0 END) as failure_count,
                                AVG(CASE WHEN was_successful AND latency_ms IS NOT NULL THEN latency_ms END) as avg_latency_ms
                            FROM health_checks 
                            GROUP BY node_id
                        ) s2 ON n2.id = s2.node_id
                        WHERE n2.network = ? AND (COALESCE(s2.success_count, 0) + COALESCE(s2.failure_count, 0)) > 0
                        ORDER BY reliability_score DESC
                        LIMIT 4
                    )
                ) reliable_nodes ON n.id = reliable_nodes.node_id
                WHERE n.network = ?
                ORDER BY RANDOM()
                LIMIT ?
                "#,
                network,
                network,
                limit
            )
            .fetch_all(&self.pool)
            .await?;

            return Ok(rows
                .into_iter()
                .map(|row| MoneroNode {
                    id: Some(row.id),
                    scheme: row.scheme,
                    host: row.host,
                    port: row.port,
                    full_url: row.full_url,
                    network: row.network,
                    first_seen_at: row.first_seen_at,
                    success_count: row.success_count,
                    failure_count: row.failure_count,
                    last_success: row.last_success,
                    last_failure: row.last_failure,
                    last_checked: row.last_checked,
                    is_reliable: row.is_reliable != 0,
                    avg_latency_ms: row.avg_latency_ms,
                    min_latency_ms: row.min_latency_ms,
                    max_latency_ms: row.max_latency_ms,
                    last_latency_ms: row.last_latency_ms,
                })
                .collect());
        }

        // If exclude_ids is not empty, we need to handle it differently
        // For now, get all nodes and filter in Rust (can be optimized with dynamic SQL)
        let fetch_limit = limit + exclude_ids.len() as i64 + 10; // Get extra to account for exclusions
        let all_rows = sqlx::query!(
            r#"
            SELECT 
                n.id as "id!: i64",
                n.scheme,
                n.host,
                n.port,
                n.full_url,
                n.network,
                n.first_seen_at,
                CAST(COALESCE(stats.success_count, 0) AS INTEGER) as "success_count!: i64",
                CAST(COALESCE(stats.failure_count, 0) AS INTEGER) as "failure_count!: i64",
                stats.last_success as "last_success?: String",
                stats.last_failure as "last_failure?: String",
                stats.last_checked as "last_checked?: String",
                CAST(CASE WHEN reliable_nodes.node_id IS NOT NULL THEN 1 ELSE 0 END AS INTEGER) as "is_reliable!: i64",
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
            LEFT JOIN (
                SELECT DISTINCT node_id FROM (
                    SELECT 
                        n2.id as node_id,
                        COALESCE(s2.success_count, 0) as success_count,
                        COALESCE(s2.failure_count, 0) as failure_count,
                        s2.avg_latency_ms,
                        (CAST(COALESCE(s2.success_count, 0) AS REAL) / CAST(COALESCE(s2.success_count, 0) + COALESCE(s2.failure_count, 0) AS REAL)) * 
                        (MIN(COALESCE(s2.success_count, 0) + COALESCE(s2.failure_count, 0), 200) / 200.0) * 0.8 +
                        CASE 
                            WHEN s2.avg_latency_ms IS NOT NULL THEN (1.0 - (MIN(s2.avg_latency_ms, 2000) / 2000.0)) * 0.2
                            ELSE 0.0 
                        END as reliability_score
                    FROM monero_nodes n2
                    LEFT JOIN (
                        SELECT 
                            node_id,
                            SUM(CASE WHEN was_successful THEN 1 ELSE 0 END) as success_count,
                            SUM(CASE WHEN NOT was_successful THEN 1 ELSE 0 END) as failure_count,
                            AVG(CASE WHEN was_successful AND latency_ms IS NOT NULL THEN latency_ms END) as avg_latency_ms
                        FROM health_checks 
                        GROUP BY node_id
                    ) s2 ON n2.id = s2.node_id
                    WHERE n2.network = ? AND (COALESCE(s2.success_count, 0) + COALESCE(s2.failure_count, 0)) > 0
                    ORDER BY reliability_score DESC
                    LIMIT 4
                )
            ) reliable_nodes ON n.id = reliable_nodes.node_id
            WHERE n.network = ?
            ORDER BY RANDOM()
            LIMIT ?
            "#,
            network,
            network,
            fetch_limit
        )
        .fetch_all(&self.pool)
        .await?;

        // Convert exclude_ids to a HashSet for O(1) lookup
        let exclude_set: std::collections::HashSet<i64> = exclude_ids.iter().cloned().collect();

        let nodes: Vec<MoneroNode> = all_rows
            .into_iter()
            .filter(|row| !exclude_set.contains(&row.id))
            .take(limit as usize)
            .map(|row| MoneroNode {
                id: Some(row.id),
                scheme: row.scheme,
                host: row.host,
                port: row.port,
                full_url: row.full_url,
                network: row.network,
                first_seen_at: row.first_seen_at,
                success_count: row.success_count,
                failure_count: row.failure_count,
                last_success: row.last_success,
                last_failure: row.last_failure,
                last_checked: row.last_checked,
                is_reliable: row.is_reliable != 0,
                avg_latency_ms: row.avg_latency_ms,
                min_latency_ms: row.min_latency_ms,
                max_latency_ms: row.max_latency_ms,
                last_latency_ms: row.last_latency_ms,
            })
            .collect();

        Ok(nodes)
    }
}

pub fn get_app_data_dir() -> Result<PathBuf> {
    let base_dir =
        data_dir().ok_or_else(|| anyhow::anyhow!("Could not determine system data directory"))?;

    let app_dir = base_dir.join("monero-rpc-pool");

    if !app_dir.exists() {
        std::fs::create_dir_all(&app_dir)?;
        info!("Created application data directory: {}", app_dir.display());
    }

    Ok(app_dir)
}
