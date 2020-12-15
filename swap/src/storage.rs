use anyhow::{anyhow, bail, Context, Result};
use model::Swap;
use serde::{de::DeserializeOwned, Serialize};
use std::path::Path;
use uuid::Uuid;

pub mod model;

pub struct Database(sled::Db);

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let db =
            sled::open(path).with_context(|| format!("Could not open the DB at {:?}", path))?;

        Ok(Database(db))
    }

    pub async fn insert_latest_state(&self, swap_id: Uuid, state: Swap) -> Result<()> {
        let key = serialize(&swap_id)?;
        let new_value = serialize(&state).context("Could not serialize new state value")?;

        let old_value = self.0.get(&key)?;

        self.0
            .compare_and_swap(key, old_value, Some(new_value))
            .context("Could not write in the DB")?
            .context("Stored swap somehow changed, aborting saving")?;

        // TODO: see if this can be done through sled config
        self.0
            .flush_async()
            .await
            .map(|_| ())
            .context("Could not flush db")
    }

    pub fn get_state(&self, swap_id: Uuid) -> anyhow::Result<Swap> {
        let key = serialize(&swap_id)?;

        let encoded = self
            .0
            .get(&key)?
            .ok_or_else(|| anyhow!("State does not exist {:?}", key))?;

        let state = deserialize(&encoded).context("Could not deserialize state")?;
        Ok(state)
    }

    pub fn all(&self) -> Result<Vec<(Uuid, Swap)>> {
        self.0
            .iter()
            .map(|item| match item {
                Ok((key, value)) => {
                    let swap_id = deserialize::<Uuid>(&key);
                    let swap = deserialize::<Swap>(&value).context("failed to deserialize swap");

                    match (swap_id, swap) {
                        (Ok(swap_id), Ok(swap)) => Ok((swap_id, swap)),
                        (Ok(_), Err(err)) => Err(err),
                        _ => bail!("failed to deserialize swap"),
                    }
                }
                Err(err) => Err(err).context("failed to retrieve swap from DB"),
            })
            .collect()
    }
}

pub fn serialize<T>(t: &T) -> anyhow::Result<Vec<u8>>
where
    T: Serialize,
{
    Ok(serde_cbor::to_vec(t)?)
}

pub fn deserialize<T>(v: &[u8]) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    Ok(serde_cbor::from_slice(&v)?)
}

#[cfg(test)]
mod tests {
    use crate::storage::model::{Alice, Bob};

    use super::*;

    #[tokio::test]
    async fn can_write_and_read_to_multiple_keys() {
        let db_dir = tempfile::tempdir().unwrap();
        let db = Database::open(db_dir.path()).unwrap();

        let state_1 = Swap::Alice(Alice::SwapComplete);
        let swap_id_1 = Uuid::new_v4();
        db.insert_latest_state(swap_id_1, state_1.clone())
            .await
            .expect("Failed to save second state");

        let state_2 = Swap::Bob(Bob::SwapComplete);
        let swap_id_2 = Uuid::new_v4();
        db.insert_latest_state(swap_id_2, state_2.clone())
            .await
            .expect("Failed to save first state");

        let recovered_1 = db
            .get_state(swap_id_1)
            .expect("Failed to recover first state");

        let recovered_2 = db
            .get_state(swap_id_2)
            .expect("Failed to recover second state");

        assert_eq!(recovered_1, state_1);
        assert_eq!(recovered_2, state_2);
    }

    #[tokio::test]
    async fn can_write_twice_to_one_key() {
        let db_dir = tempfile::tempdir().unwrap();
        let db = Database::open(db_dir.path()).unwrap();

        let state = Swap::Alice(Alice::SwapComplete);

        let swap_id = Uuid::new_v4();
        db.insert_latest_state(swap_id, state.clone())
            .await
            .expect("Failed to save state the first time");
        let recovered = db
            .get_state(swap_id)
            .expect("Failed to recover state the first time");

        // We insert and recover twice to ensure database implementation allows the
        // caller to write to an existing key
        db.insert_latest_state(swap_id, recovered)
            .await
            .expect("Failed to save state the second time");
        let recovered = db
            .get_state(swap_id)
            .expect("Failed to recover state the second time");

        assert_eq!(recovered, state);
    }

    #[tokio::test]
    async fn can_fetch_all_keys() {
        let db_dir = tempfile::tempdir().unwrap();
        let db = Database::open(db_dir.path()).unwrap();

        let state_1 = Swap::Alice(Alice::SwapComplete);
        let swap_id_1 = Uuid::new_v4();
        db.insert_latest_state(swap_id_1, state_1.clone())
            .await
            .expect("Failed to save second state");

        let state_2 = Swap::Bob(Bob::SwapComplete);
        let swap_id_2 = Uuid::new_v4();
        db.insert_latest_state(swap_id_2, state_2.clone())
            .await
            .expect("Failed to save first state");

        let swaps = db.all().unwrap();

        assert_eq!(swaps.len(), 2);
        assert!(swaps.contains(&(swap_id_1, state_1)));
        assert!(swaps.contains(&(swap_id_2, state_2)));
    }
}
