pub use alice::Alice;
pub use bob::Bob;

use async_trait::async_trait;
use anyhow::{anyhow, Context, Result};
use itertools::Itertools;
use libp2p::{Multiaddr, PeerId};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Debug};
use std::path::Path;
use std::str::FromStr;
use uuid::Uuid;
use crate::protocol::alice::AliceState;
use crate::protocol::bob::BobState;
use crate::protocol::{Database, State};
use std::collections::HashMap;


mod alice;
mod bob;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum Swap {
    Alice(Alice),
    Bob(Bob),
}

impl From<State> for Swap {
    fn from(state: State) -> Self {
        match state {
            State::Alice(state) => Swap::Alice(state.into()),
            State::Bob(state) => Swap::Bob(state.into()),
        }
    }
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

impl Display for Swap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Swap::Alice(alice) => Display::fmt(alice, f),
            Swap::Bob(bob) => Display::fmt(bob, f),
        }
    }
}

impl From<Swap> for State {
    fn from(value: Swap) -> Self {
        match value {
            Swap::Alice(alice) => State::Alice(alice.into()),
            Swap::Bob(bob) => State::Bob(bob.into()),
        }
    }
}

impl From<BobState> for Swap {
    fn from(state: BobState) -> Self {
        Self::Bob(Bob::from(state))
    }
}

impl From<AliceState> for Swap {
    fn from(state: AliceState) -> Self {
        Self::Alice(Alice::from(state))
    }
}

#[derive(Clone)]
pub struct SledDatabase  {
    swaps: sled::Tree,
    peers: sled::Tree,
    addresses: sled::Tree,
    monero_addresses: sled::Tree,
}

#[async_trait]
impl Database for SledDatabase {
    async fn insert_peer_id(&self, swap_id: Uuid, peer_id: PeerId) -> Result<()> {
        let peer_id_str = peer_id.to_string();

        let key = serialize(&swap_id)?;
        let value = serialize(&peer_id_str).context("Could not serialize peer-id")?;

        self.peers.insert(key, value)?;

        self.peers
            .flush_async()
            .await
            .map(|_| ())
            .context("Could not flush db")
    }

    async fn get_peer_id(&self, swap_id: Uuid) -> Result<PeerId> {
        let key = serialize(&swap_id)?;

        let encoded = self
            .peers
            .get(&key)?
            .ok_or_else(|| anyhow!("No peer-id found for swap id {} in database", swap_id))?;

        let peer_id: String = deserialize(&encoded).context("Could not deserialize peer-id")?;
        Ok(PeerId::from_str(peer_id.as_str())?)
    }

    async fn insert_monero_address(
        &self,
        swap_id: Uuid,
        address: monero::Address,
    ) -> Result<()> {
        let key = swap_id.as_bytes();
        let value = serialize(&address)?;

        self.monero_addresses.insert(key, value)?;

        self.monero_addresses
            .flush_async()
            .await
            .map(|_| ())
            .context("Could not flush db")
    }

    async fn get_monero_address(&self, swap_id: Uuid) -> Result<monero::Address> {
        let encoded = self
            .monero_addresses
            .get(swap_id.as_bytes())?
            .ok_or_else(|| {
                anyhow!(
                    "No Monero address found for swap id {} in database",
                    swap_id
                )
            })?;

        let monero_address = deserialize(&encoded)?;

        Ok(monero_address)
    }

    async fn insert_address(&self, peer_id: PeerId, address: Multiaddr) -> Result<()> {
        let key = peer_id.to_bytes();

        let existing_addresses = self.addresses.get(&key)?;

        let new_addresses = {
            let existing_addresses = existing_addresses.clone();

            Some(match existing_addresses {
                Some(encoded) => {
                    let mut addresses = deserialize::<Vec<Multiaddr>>(&encoded)?;
                    addresses.push(address);

                    serialize(&addresses)?
                }
                None => serialize(&[address])?,
            })
        };

        self.addresses
            .compare_and_swap(key, existing_addresses, new_addresses)??;

        self.addresses
            .flush_async()
            .await
            .map(|_| ())
            .context("Could not flush db")
    }

    async fn get_addresses(&self, peer_id: PeerId) -> Result<Vec<Multiaddr>> {
        let key = peer_id.to_bytes();

        let addresses = match self.addresses.get(&key)? {
            Some(encoded) => deserialize(&encoded).context("Failed to deserialize addresses")?,
            None => vec![],
        };

        Ok(addresses)
    }

    async fn insert_latest_state(&self, swap_id: Uuid, state: State) -> Result<()> {
        let key = serialize(&swap_id)?;
        let swap =  Swap::from(state);
        let new_value = serialize(&swap).context("Could not serialize new state value")?;

        let old_value = self.swaps.get(&key)?;

        self.swaps
            .compare_and_swap(key, old_value, Some(new_value))
            .context("Could not write in the DB")?
            .context("Stored swap somehow changed, aborting saving")?;

        self.swaps
            .flush_async()
            .await
            .map(|_| ())
            .context("Could not flush db")
    }

    async fn get_state(&self, swap_id: Uuid) -> Result<State> {
        let key = serialize(&swap_id)?;

        let encoded = self
            .swaps
            .get(&key)?
            .ok_or_else(|| anyhow!("Swap with id {} not found in database", swap_id))?;

        let swap = deserialize::<Swap>(&encoded).context("Could not deserialize state")?;

        let state = State::from(swap);

        Ok(state)
    }

    async fn all(&self) -> Result<Vec<(Uuid, State)>> {
        self.all_iter().collect()
    }

    async fn unfinished(&self, unfinished: fn(State) -> bool) -> Result<HashMap<Uuid, State>> {
        self.all_iter().into_iter()
            .filter_ok(|(_swap_id, state)| unfinished(state.clone()))
            .collect()
    }
}

impl SledDatabase {
    pub async fn open(path: &Path) -> Result<Self> {
        tracing::debug!("Opening database at {}", path.display());

        let db =
            sled::open(path).with_context(|| format!("Could not open the DB at {:?}", path))?;

        let swaps = db.open_tree("swaps")?;
        let peers = db.open_tree("peers")?;
        let addresses = db.open_tree("addresses")?;
        let monero_addresses = db.open_tree("monero_addresses")?;

        Ok(SledDatabase {
            swaps,
            peers,
            addresses,
            monero_addresses,
        })
    }

    fn all_iter(&self) -> impl Iterator<Item = Result<(Uuid, State)>> {
        self.swaps.iter().map(|item| {
            let (key, value) = item.context("Failed to retrieve swap from DB")?;

            let swap_id = deserialize::<Uuid>(&key)?;
            let swap = deserialize::<Swap>(&value).context("Failed to deserialize swap")?;

            let state = State::from(swap);

            Ok((swap_id, state))
        })
    }
}

pub fn serialize<T>(t: &T) -> Result<Vec<u8>>
where
    T: Serialize,
{
    Ok(serde_cbor::to_vec(t)?)
}

pub fn deserialize<T>(v: &[u8]) -> Result<T>
where
    T: DeserializeOwned,
{
    Ok(serde_cbor::from_slice(&v)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::alice::AliceState;

    #[tokio::test]
    async fn can_write_and_read_to_multiple_keys() {
        let db_dir = tempfile::tempdir().unwrap();
        let db = SledDatabase::open(db_dir.path()).await.unwrap();

        let state_1 = State::from(AliceState::BtcRedeemed);
        let swap_id_1 = Uuid::new_v4();
        db.insert_latest_state(swap_id_1, state_1.clone())
            .await
            .expect("Failed to save second state");

        let state_2 = State::from(AliceState::BtcPunished);
        let swap_id_2 = Uuid::new_v4();
        db.insert_latest_state(swap_id_2, state_2.clone())
            .await
            .expect("Failed to save first state");

        let recovered_1 = db
            .get_state(swap_id_1)
            .await
            .expect("Failed to recover first state");

        let recovered_2 = db
            .get_state(swap_id_2)
            .await
            .expect("Failed to recover second state");

        assert_eq!(recovered_1, state_1);
        assert_eq!(recovered_2, state_2);
    }

    #[tokio::test]
    async fn can_write_twice_to_one_key() {
        let db_dir = tempfile::tempdir().unwrap();
        let db = SledDatabase::open(db_dir.path()).await.unwrap();

        let state = State::from(AliceState::SafelyAborted);

        let swap_id = Uuid::new_v4();
        db.insert_latest_state(swap_id, state.clone())
            .await
            .expect("Failed to save state the first time");
        let recovered = db
            .get_state(swap_id)
            .await
            .expect("Failed to recover state the first time");

        // We insert and recover twice to ensure database implementation allows the
        // caller to write to an existing key
        db.insert_latest_state(swap_id, recovered)
            .await
            .expect("Failed to save state the second time");
        let recovered = db
            .get_state(swap_id)
            .await
            .expect("Failed to recover state the second time");

        assert_eq!(recovered, state);
    }

    #[tokio::test]
    async fn can_save_swap_state_and_peer_id_with_same_swap_id() -> Result<()> {
        let db_dir = tempfile::tempdir().unwrap();
        let db = SledDatabase::open(db_dir.path()).await.unwrap();

        let alice_id = Uuid::new_v4();
        let alice_state = State::from(AliceState::BtcPunished);
        let peer_id = PeerId::random();

        db.insert_latest_state(alice_id, alice_state.clone()).await?;
        db.insert_peer_id(alice_id, peer_id).await?;

        let loaded_swap = db.get_state(alice_id).await?;
        let loaded_peer_id = db.get_peer_id(alice_id).await?;

        assert_eq!(alice_state, loaded_swap);
        assert_eq!(peer_id, loaded_peer_id);

        Ok(())
    }

    #[tokio::test]
    async fn test_reopen_db() -> Result<()> {
        let db_dir = tempfile::tempdir().unwrap();
        let alice_id = Uuid::new_v4();
        let alice_state = State::from(AliceState::BtcPunished);

        let peer_id = PeerId::random();

        {
            let db = SledDatabase::open(db_dir.path()).await.unwrap();
            db.insert_latest_state(alice_id,alice_state.clone()).await?;
            db.insert_peer_id(alice_id, peer_id).await?;
        }

        let db = SledDatabase::open(db_dir.path()).await.unwrap();

        let loaded_swap = db.get_state(alice_id).await?;
        let loaded_peer_id = db.get_peer_id(alice_id).await?;

        assert_eq!(alice_state, loaded_swap);
        assert_eq!(peer_id, loaded_peer_id);

        Ok(())
    }

    #[tokio::test]
    async fn save_and_load_addresses() -> Result<()> {
        let db_dir = tempfile::tempdir()?;
        let peer_id = PeerId::random();
        let home1 = "/ip4/127.0.0.1/tcp/1".parse::<Multiaddr>()?;
        let home2 = "/ip4/127.0.0.1/tcp/2".parse::<Multiaddr>()?;

        {
            let db = SledDatabase::open(db_dir.path()).await?;
            db.insert_address(peer_id, home1.clone()).await?;
            db.insert_address(peer_id, home2.clone()).await?;
        }

        let addresses = SledDatabase::open(db_dir.path()).await?.get_addresses(peer_id).await?;

        assert_eq!(addresses, vec![home1, home2]);

        Ok(())
    }

    #[tokio::test]
    async fn save_and_load_monero_address() -> Result<()> {
        let db_dir = tempfile::tempdir()?;
        let swap_id = Uuid::new_v4();

        SledDatabase::open(db_dir.path()).await?.insert_monero_address(swap_id, "53gEuGZUhP9JMEBZoGaFNzhwEgiG7hwQdMCqFxiyiTeFPmkbt1mAoNybEUvYBKHcnrSgxnVWgZsTvRBaHBNXPa8tHiCU51a".parse()?).await?;
        let loaded_monero_address = SledDatabase::open(db_dir.path()).await?.get_monero_address(swap_id).await?;

        assert_eq!(loaded_monero_address.to_string(), "53gEuGZUhP9JMEBZoGaFNzhwEgiG7hwQdMCqFxiyiTeFPmkbt1mAoNybEUvYBKHcnrSgxnVWgZsTvRBaHBNXPa8tHiCU51a");

        Ok(())
    }
}
