use anyhow::{anyhow, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;
use xmr_btc::{alice, bob, monero, serde::monero_private_key};

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum Swap {
    Alice(Alice),
    Bob(Bob),
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum Alice {
    Handshaken(alice::State3),
    BtcLocked(alice::State3),
    XmrLocked(alice::State3),
    BtcRedeemable {
        state: alice::State3,
        redeem_tx: bitcoin::Transaction,
    },
    BtcPunishable(alice::State3),
    BtcRefunded {
        state: alice::State3,
        #[serde(with = "monero_private_key")]
        spend_key: monero::PrivateKey,
        view_key: monero::PrivateViewKey,
    },
    SwapComplete,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum Bob {
    Handshaken(bob::State2),
    BtcLocked(bob::State2),
    XmrLocked(bob::State2),
    BtcRedeemed(bob::State2),
    BtcRefundable(bob::State2),
    SwapComplete,
}

impl From<Alice> for Swap {
    fn from(from: Alice) -> Self {
        Swap::Alice(from)
    }
}

impl From<Bob> for Swap {
    fn from(from: Bob) -> Self {
        Swap::Bob(from)
    }
}

pub struct Database(sled::Db);

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let db =
            sled::open(path).with_context(|| format!("Could not open the DB at {:?}", path))?;

        Ok(Database(db))
    }

    // TODO: Add method to update state

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

    pub fn get_latest_state(&self, swap_id: Uuid) -> anyhow::Result<Swap> {
        let key = serialize(&swap_id)?;

        let encoded = self
            .0
            .get(&key)?
            .ok_or_else(|| anyhow!("State does not exist {:?}", key))?;

        let state = deserialize(&encoded).context("Could not deserialize state")?;
        Ok(state)
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
            .get_latest_state(swap_id_1)
            .expect("Failed to recover first state");

        let recovered_2 = db
            .get_latest_state(swap_id_2)
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
            .get_latest_state(swap_id)
            .expect("Failed to recover state the first time");

        // We insert and recover twice to ensure database implementation allows the
        // caller to write to an existing key
        db.insert_latest_state(swap_id, recovered)
            .await
            .expect("Failed to save state the second time");
        let recovered = db
            .get_latest_state(swap_id)
            .expect("Failed to recover state the second time");

        assert_eq!(recovered, state);
    }
}
