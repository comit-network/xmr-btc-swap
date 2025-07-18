use std::path::PathBuf;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tracing::info;

#[derive(Clone)]
pub struct Database {
    pub pool: SqlitePool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentWallet {
    pub wallet_path: String,
    pub last_opened_at: DateTime<Utc>,
}

impl Database {
    pub async fn new(data_dir: PathBuf) -> Result<Self> {
        if !data_dir.exists() {
            std::fs::create_dir_all(&data_dir)?;
            info!("Created wallet database directory: {}", data_dir.display());
        }

        let db_path = data_dir.join("recent_wallets.db");
        let database_url = format!("sqlite:{}?mode=rwc", db_path.display());
        let pool = SqlitePool::connect(&database_url).await?;

        let db = Self { pool };
        db.migrate().await?;

        Ok(db)
    }

    async fn migrate(&self) -> Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        info!("Recent wallets database migration completed");
        Ok(())
    }

    /// Record that a wallet was accessed
    pub async fn record_wallet_access(&self, wallet_path: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();

        sqlx::query!(
            r#"
            INSERT INTO recent_wallets (wallet_path, last_opened_at)
            VALUES (?, ?)
            ON CONFLICT(wallet_path) DO UPDATE SET last_opened_at = excluded.last_opened_at
            "#,
            wallet_path,
            now
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get recently opened wallets, most recent first
    pub async fn get_recent_wallets(&self, limit: i64) -> Result<Vec<RecentWallet>> {
        let rows = sqlx::query!(
            r#"
            SELECT wallet_path, last_opened_at
            FROM recent_wallets 
            ORDER BY last_opened_at DESC
            LIMIT ?
            "#,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        let wallets: Vec<RecentWallet> = rows
            .into_iter()
            .map(|row| RecentWallet {
                wallet_path: row.wallet_path,
                last_opened_at: row.last_opened_at.parse().unwrap_or_else(|_| Utc::now()),
            })
            .collect();

        Ok(wallets)
    }
}
