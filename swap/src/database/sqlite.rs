use crate::database::Swap;
use crate::monero::Address;
use crate::protocol::{Database, State};
use anyhow::{Context, Result};
use async_trait::async_trait;
use libp2p::{Multiaddr, PeerId};
use sqlx::sqlite::Sqlite;
use sqlx::{Pool, SqlitePool};
use std::path::Path;
use std::str::FromStr;
use time::OffsetDateTime;
use uuid::Uuid;

pub struct SqliteDatabase {
    pool: Pool<Sqlite>,
}

impl SqliteDatabase {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self>
    where
        Self: std::marker::Sized,
    {
        let path_str = format!("sqlite:{}", path.as_ref().display());
        let pool = SqlitePool::connect(&path_str).await?;
        let mut sqlite = Self { pool };
        sqlite.run_migrations().await?;
        Ok(sqlite)
    }

    async fn run_migrations(&mut self) -> anyhow::Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }
}

#[async_trait]
impl Database for SqliteDatabase {
    async fn insert_peer_id(&self, swap_id: Uuid, peer_id: PeerId) -> Result<()> {
        let mut conn = self.pool.acquire().await?;

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
        .execute(&mut conn)
        .await?;

        Ok(())
    }

    async fn get_peer_id(&self, swap_id: Uuid) -> Result<PeerId> {
        let mut conn = self.pool.acquire().await?;

        let swap_id = swap_id.to_string();

        let row = sqlx::query!(
            r#"
        SELECT peer_id
        FROM peers
        WHERE swap_id = ?
        "#,
            swap_id
        )
        .fetch_one(&mut conn)
        .await?;

        let peer_id = PeerId::from_str(&row.peer_id)?;
        Ok(peer_id)
    }

    async fn insert_monero_address(&self, swap_id: Uuid, address: Address) -> Result<()> {
        let mut conn = self.pool.acquire().await?;

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
        .execute(&mut conn)
        .await?;

        Ok(())
    }

    async fn get_monero_address(&self, swap_id: Uuid) -> Result<Address> {
        let mut conn = self.pool.acquire().await?;

        let swap_id = swap_id.to_string();

        let row = sqlx::query!(
            r#"
        SELECT address
        FROM monero_addresses
        WHERE swap_id = ?
        "#,
            swap_id
        )
        .fetch_one(&mut conn)
        .await?;

        let address = row.address.parse()?;

        Ok(address)
    }

    async fn insert_address(&self, peer_id: PeerId, address: Multiaddr) -> Result<()> {
        let mut conn = self.pool.acquire().await?;

        let peer_id = peer_id.to_string();
        let address = address.to_string();

        sqlx::query!(
            r#"
        insert into peer_addresses (
            peer_id,
            address
            ) values (?, ?);
        "#,
            peer_id,
            address
        )
        .execute(&mut conn)
        .await?;

        Ok(())
    }

    async fn get_addresses(&self, peer_id: PeerId) -> Result<Vec<Multiaddr>> {
        let mut conn = self.pool.acquire().await?;

        let peer_id = peer_id.to_string();

        let rows = sqlx::query!(
            r#"
        SELECT address
        FROM peer_addresses
        WHERE peer_id = ?
        "#,
            peer_id,
        )
        .fetch_all(&mut conn)
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

    async fn insert_latest_state(&self, swap_id: Uuid, state: State) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        let entered_at = OffsetDateTime::now_utc();

        let swap_id = swap_id.to_string();
        let swap = serde_json::to_string(&Swap::from(state))?;
        let entered_at = entered_at.to_string();

        sqlx::query!(
            r#"
            insert into swap_states (
                swap_id,
                entered_at,
                state
                ) values (?, ?, ?);
        "#,
            swap_id,
            entered_at,
            swap
        )
        .execute(&mut conn)
        .await?;

        Ok(())
    }

    async fn get_state(&self, swap_id: Uuid) -> Result<State> {
        let mut conn = self.pool.acquire().await?;
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
        .fetch_all(&mut conn)
        .await?;

        let row = row
            .first()
            .context(format!("No state in database for swap: {}", swap_id))?;
        let swap: Swap = serde_json::from_str(&row.state)?;

        Ok(swap.into())
    }

    async fn all(&self) -> Result<Vec<(Uuid, State)>> {
        let mut conn = self.pool.acquire().await?;
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
        .fetch_all(&mut conn)
        .await?;

        let result = rows
            .iter()
            .map(|row| {
                let swap_id = Uuid::from_str(&row.swap_id)?;
                let state = match serde_json::from_str::<Swap>(&row.state) {
                    Ok(a) => Ok(State::from(a)),
                    Err(e) => Err(e),
                }?;
                Ok((swap_id, state))
            })
            .collect::<Result<Vec<(Uuid, State)>>>();

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::alice::AliceState;
    use crate::protocol::bob::BobState;
    use std::fs::File;
    use tempfile::tempdir;

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
        let state_2 = State::Alice(AliceState::BtcPunished);
        let state_3 = State::Alice(AliceState::SafelyAborted);
        let state_4 = State::Bob(BobState::SafelyAborted);
        let swap_id_1 = Uuid::new_v4();
        let swap_id_2 = Uuid::new_v4();

        db.insert_latest_state(swap_id_1, state_1.clone())
            .await
            .unwrap();
        db.insert_latest_state(swap_id_1, state_2.clone())
            .await
            .unwrap();
        db.insert_latest_state(swap_id_1, state_3.clone())
            .await
            .unwrap();
        db.insert_latest_state(swap_id_2, state_4.clone())
            .await
            .unwrap();

        let latest_loaded = db.all().await.unwrap();

        assert_eq!(latest_loaded.len(), 2);

        assert!(latest_loaded.contains(&(swap_id_1, state_3)));
        assert!(latest_loaded.contains(&(swap_id_2, state_4)));

        assert!(!latest_loaded.contains(&(swap_id_1, state_1)));
        assert!(!latest_loaded.contains(&(swap_id_1, state_2)));
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
        let temp_db = tempdir().unwrap().into_path().join("tempdb");

        // file has to exist in order to connect with sqlite
        File::create(temp_db.clone()).unwrap();

        let db = SqliteDatabase::open(temp_db).await?;

        Ok(db)
    }
}
