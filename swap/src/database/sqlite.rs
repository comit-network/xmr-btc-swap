use crate::cli::api::tauri_bindings::TauriEmitter;
use crate::cli::api::tauri_bindings::TauriHandle;
use crate::database::Swap;
use crate::monero::{Address, TransferProof};
use crate::protocol::{Database, State};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use libp2p::{Multiaddr, PeerId};
use sqlx::sqlite::{Sqlite, SqliteConnectOptions};
use sqlx::{ConnectOptions, Pool, SqlitePool};
use std::path::Path;
use std::str::FromStr;
use time::OffsetDateTime;
use uuid::Uuid;

use super::AccessMode;

pub struct SqliteDatabase {
    pool: Pool<Sqlite>,
    tauri_handle: Option<TauriHandle>,
}

impl SqliteDatabase {
    pub async fn open(path: impl AsRef<Path>, access_mode: AccessMode) -> Result<Self>
    where
        Self: std::marker::Sized,
    {
        let read_only = matches!(access_mode, AccessMode::ReadOnly);

        let path_str = format!("sqlite:{}", path.as_ref().display());

        let options = SqliteConnectOptions::from_str(&path_str)?.read_only(read_only);
        let options = options.disable_statement_logging();

        let pool = SqlitePool::connect_with(options.to_owned()).await?;
        let mut sqlite = Self {
            pool,
            tauri_handle: None,
        };

        if !read_only {
            sqlite.run_migrations().await?;
        }

        Ok(sqlite)
    }

    pub fn with_tauri_handle(mut self, tauri_handle: impl Into<Option<TauriHandle>>) -> Self {
        self.tauri_handle = tauri_handle.into();
        self
    }

    async fn run_migrations(&mut self) -> anyhow::Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }
}

#[async_trait]
impl Database for SqliteDatabase {
    async fn insert_peer_id(&self, swap_id: Uuid, peer_id: PeerId) -> Result<()> {
        let swap_id = swap_id.to_string();
        let peer_id = peer_id.to_string();

        sqlx::query!(
            r#"
        insert into peers (
            swap_id,
            peer_id
            ) values (?, ?);
        "#,
            swap_id,
            peer_id
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_peer_id(&self, swap_id: Uuid) -> Result<PeerId> {
        let swap_id = swap_id.to_string();

        let row = sqlx::query!(
            r#"
        SELECT peer_id
        FROM peers
        WHERE swap_id = ?
        "#,
            swap_id
        )
        .fetch_one(&self.pool)
        .await?;

        let peer_id = PeerId::from_str(&row.peer_id)?;
        Ok(peer_id)
    }

    async fn insert_monero_address(&self, swap_id: Uuid, address: Address) -> Result<()> {
        let swap_id = swap_id.to_string();
        let address = address.to_string();

        sqlx::query!(
            r#"
        insert into monero_addresses (
            swap_id,
            address
            ) values (?, ?);
        "#,
            swap_id,
            address
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_monero_address(&self, swap_id: Uuid) -> Result<Address> {
        let swap_id = swap_id.to_string();

        let row = sqlx::query!(
            r#"
        SELECT address
        FROM monero_addresses
        WHERE swap_id = ?
        "#,
            swap_id
        )
        .fetch_one(&self.pool)
        .await?;

        let address = row.address.parse()?;

        Ok(address)
    }

    async fn get_monero_addresses(&self) -> Result<Vec<monero::Address>> {
        let rows = sqlx::query!("SELECT DISTINCT address FROM monero_addresses")
            .fetch_all(&self.pool)
            .await?;

        let addresses = rows
            .iter()
            .map(|row| row.address.parse())
            .collect::<Result<Vec<_>, _>>()?;

        Ok(addresses)
    }

    async fn insert_address(&self, peer_id: PeerId, address: Multiaddr) -> Result<()> {
        let peer_id = peer_id.to_string();
        let address = address.to_string();

        sqlx::query!(
            r#"
        insert or ignore into peer_addresses (
            peer_id,
            address
            ) values (?, ?);
        "#,
            peer_id,
            address
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_addresses(&self, peer_id: PeerId) -> Result<Vec<Multiaddr>> {
        let peer_id = peer_id.to_string();

        let rows = sqlx::query!(
            r#"
        SELECT DISTINCT address
        FROM peer_addresses
        WHERE peer_id = ?
        "#,
            peer_id,
        )
        .fetch_all(&self.pool)
        .await?;

        let addresses = rows
            .iter()
            .map(|row| {
                let multiaddr = Multiaddr::from_str(&row.address)?;
                Ok(multiaddr)
            })
            .collect::<Result<Vec<Multiaddr>>>();

        addresses
    }

    async fn get_all_peer_addresses(&self) -> Result<Vec<(PeerId, Vec<Multiaddr>)>> {
        let rows = sqlx::query!("SELECT peer_id, address FROM peer_addresses")
            .fetch_all(&self.pool)
            .await?;

        let mut peer_map: std::collections::HashMap<PeerId, Vec<Multiaddr>> =
            std::collections::HashMap::new();

        for row in rows.iter() {
            match (
                PeerId::from_str(&row.peer_id),
                Multiaddr::from_str(&row.address),
            ) {
                (Ok(peer_id), Ok(multiaddr)) => {
                    peer_map.entry(peer_id).or_default().push(multiaddr);
                }
                (Err(e), _) => {
                    tracing::warn!(
                        peer_id = %row.peer_id,
                        error = %e,
                        "Failed to parse peer ID, skipping entry"
                    );
                }
                (_, Err(e)) => {
                    tracing::warn!(
                        address = %row.address,
                        error = %e,
                        "Failed to parse multiaddr, skipping entry"
                    );
                }
            }
        }

        Ok(peer_map.into_iter().collect())
    }

    async fn get_swap_start_date(&self, swap_id: Uuid) -> Result<String> {
        let swap_id = swap_id.to_string();

        let row = sqlx::query!(
            r#"
                SELECT min(entered_at) as start_date
                FROM swap_states
                WHERE swap_id = ?
                "#,
            swap_id
        )
        .fetch_one(&self.pool)
        .await?;

        row.start_date
            .ok_or_else(|| anyhow!("Could not get swap start date"))
    }

    async fn insert_latest_state(&self, swap_id: Uuid, state: State) -> Result<()> {
        let entered_at = OffsetDateTime::now_utc();

        let swap = serde_json::to_string(&Swap::from(state))?;
        let entered_at = entered_at.to_string();
        let swap_id_str = swap_id.to_string();

        sqlx::query!(
            r#"
            insert into swap_states (
                swap_id,
                entered_at,
                state
                ) values (?, ?, ?);
        "#,
            swap_id_str,
            entered_at,
            swap
        )
        .execute(&self.pool)
        .await?;

        // Emit event to Tauri, the frontend will then send another request to get the latest state
        // This is why we don't send the state here
        self.tauri_handle.emit_swap_state_change_event(swap_id);

        Ok(())
    }

    async fn get_state(&self, swap_id: Uuid) -> Result<State> {
        let swap_id = swap_id.to_string();
        let row = sqlx::query!(
            r#"
           SELECT state
           FROM swap_states
           WHERE swap_id = ?
           ORDER BY id desc
           LIMIT 1;

        "#,
            swap_id
        )
        .fetch_all(&self.pool)
        .await?;

        let row = row
            .first()
            .context(format!("No state in database for swap: {}", swap_id))?;
        let swap: Swap = serde_json::from_str(&row.state)?;

        Ok(swap.into())
    }

    async fn all(&self) -> Result<Vec<(Uuid, State)>> {
        let rows = sqlx::query!(
            r#"
            SELECT swap_id, state
            FROM (
                SELECT max(id), swap_id, state
                FROM swap_states
                GROUP BY swap_id
            )
        "#
        )
        .fetch_all(&self.pool)
        .await?;

        let result = rows
            .iter()
            .filter_map(|row| {
                let (Some(swap_id), Some(state)) = (&row.swap_id, &row.state) else {
                    tracing::error!("Row didn't contain state or swap_id when it should have");
                    return None;
                };

                let swap_id = match Uuid::from_str(swap_id) {
                    Ok(id) => id,
                    Err(e) => {
                        tracing::error!(%swap_id, error = ?e, "Failed to parse UUID");
                        return None;
                    }
                };
                let state = match serde_json::from_str::<Swap>(state) {
                    Ok(a) => State::from(a),
                    Err(e) => {
                        tracing::error!(%swap_id, error = ?e, "Failed to deserialize state");
                        return None;
                    }
                };

                Some((swap_id, state))
            })
            .collect::<Vec<(Uuid, State)>>();

        Ok(result)
    }

    async fn get_states(&self, swap_id: Uuid) -> Result<Vec<State>> {
        let swap_id = swap_id.to_string();

        // TODO: We should use query! instead of query here to allow for at-compile-time validation
        // I didn't manage to generate the mappings for the query! macro because of problems with sqlx-cli
        let rows = sqlx::query!(
            r#"
           SELECT state
           FROM swap_states
           WHERE swap_id = ?
        "#,
            swap_id
        )
        .fetch_all(&self.pool)
        .await?;

        let result = rows
            .iter()
            .map(|row| {
                let state_str: &str = &row.state;

                let state = match serde_json::from_str::<Swap>(state_str) {
                    Ok(a) => Ok(State::from(a)),
                    Err(e) => Err(e),
                }?;
                Ok(state)
            })
            .collect::<Result<Vec<State>>>();

        result
    }

    async fn insert_buffered_transfer_proof(
        &self,
        swap_id: Uuid,
        proof: TransferProof,
    ) -> Result<()> {
        let swap_id = swap_id.to_string();
        let proof = serde_json::to_string(&proof)?;

        sqlx::query!(
            r#"
            INSERT INTO buffered_transfer_proofs (
                swap_id,
                proof
                ) VALUES (?, ?);
        "#,
            swap_id,
            proof
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_buffered_transfer_proof(&self, swap_id: Uuid) -> Result<Option<TransferProof>> {
        let swap_id = swap_id.to_string();

        let row = sqlx::query!(
            r#"
           SELECT proof
           FROM buffered_transfer_proofs
           WHERE swap_id = ?
            "#,
            swap_id
        )
        .fetch_all(&self.pool)
        .await?;

        if row.is_empty() {
            return Ok(None);
        }

        let proof_str = &row[0].proof;
        let proof = serde_json::from_str(proof_str)?;

        Ok(Some(proof))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::alice::AliceState;
    use crate::protocol::bob::BobState;
    use std::fs::File;
    use tempfile::{tempdir, TempDir};

    #[tokio::test]
    async fn test_insert_and_load_state() {
        let db = setup_test_db().await.unwrap();

        let state_1 = State::Alice(AliceState::BtcRedeemed);
        let swap_id_1 = Uuid::new_v4();

        db.insert_latest_state(swap_id_1, state_1).await.unwrap();

        let state_1 = State::Alice(AliceState::BtcRedeemed);

        db.insert_latest_state(swap_id_1, state_1.clone())
            .await
            .unwrap();

        let state_1_loaded = db.get_state(swap_id_1).await.unwrap();

        assert_eq!(state_1, state_1_loaded);
    }

    #[tokio::test]
    async fn test_retrieve_all_latest_states() {
        let db = setup_test_db().await.unwrap();

        let state_1 = State::Alice(AliceState::BtcRedeemed);
        let state_2 = State::Alice(AliceState::SafelyAborted);
        let state_3 = State::Bob(BobState::SafelyAborted);
        let swap_id_1 = Uuid::new_v4();
        let swap_id_2 = Uuid::new_v4();

        db.insert_latest_state(swap_id_1, state_1.clone())
            .await
            .unwrap();
        db.insert_latest_state(swap_id_1, state_2.clone())
            .await
            .unwrap();
        db.insert_latest_state(swap_id_2, state_3.clone())
            .await
            .unwrap();

        let latest_loaded = db.all().await.unwrap();

        assert_eq!(latest_loaded.len(), 2);

        assert!(latest_loaded.contains(&(swap_id_1, state_2)));
        assert!(latest_loaded.contains(&(swap_id_2, state_3)));

        assert!(!latest_loaded.contains(&(swap_id_1, state_1)));
    }

    #[tokio::test]
    async fn test_insert_load_monero_address() -> Result<()> {
        let db = setup_test_db().await?;

        let swap_id = Uuid::new_v4();
        let monero_address = "53gEuGZUhP9JMEBZoGaFNzhwEgiG7hwQdMCqFxiyiTeFPmkbt1mAoNybEUvYBKHcnrSgxnVWgZsTvRBaHBNXPa8tHiCU51a".parse()?;

        db.insert_monero_address(swap_id, monero_address).await?;

        let loaded_monero_address = db.get_monero_address(swap_id).await?;

        assert_eq!(monero_address, loaded_monero_address);

        Ok(())
    }

    #[tokio::test]
    async fn test_insert_and_load_multiaddr() -> Result<()> {
        let db = setup_test_db().await?;

        let peer_id = PeerId::random();
        let multiaddr1 = "/ip4/127.0.0.1".parse::<Multiaddr>()?;
        let multiaddr2 = "/ip4/127.0.0.2".parse::<Multiaddr>()?;

        db.insert_address(peer_id, multiaddr1.clone()).await?;
        db.insert_address(peer_id, multiaddr2.clone()).await?;

        let loaded_multiaddr = db.get_addresses(peer_id).await?;

        assert!(loaded_multiaddr.contains(&multiaddr1));
        assert!(loaded_multiaddr.contains(&multiaddr2));
        assert_eq!(loaded_multiaddr.len(), 2);

        Ok(())
    }

    #[tokio::test]
    async fn test_insert_and_load_peer_id() -> Result<()> {
        let db = setup_test_db().await?;

        let peer_id = PeerId::random();
        let multiaddr1 = "/ip4/127.0.0.1".parse::<Multiaddr>()?;
        let multiaddr2 = "/ip4/127.0.0.2".parse::<Multiaddr>()?;

        db.insert_address(peer_id, multiaddr1.clone()).await?;
        db.insert_address(peer_id, multiaddr2.clone()).await?;

        let loaded_multiaddr = db.get_addresses(peer_id).await?;

        assert!(loaded_multiaddr.contains(&multiaddr1));
        assert!(loaded_multiaddr.contains(&multiaddr2));
        assert_eq!(loaded_multiaddr.len(), 2);

        Ok(())
    }

    async fn setup_test_db() -> Result<SqliteDatabase> {
        let dir: TempDir = tempdir().unwrap();
        let temp_db = dir.path().join("tempdb");

        // file has to exist in order to connect with sqlite
        File::create(&temp_db).unwrap();

        // keep the directory alive for the duration of the test
        let _db_dir = dir.keep();

        let db = SqliteDatabase::open(temp_db, AccessMode::ReadWrite).await?;

        Ok(db)
    }
}
