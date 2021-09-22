use crate::database::{Alice, Bob, Swap};
use anyhow::{anyhow, Context, Result};
use itertools::Itertools;
use libp2p::{Multiaddr, PeerId};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::Path;
use std::str::FromStr;
use uuid::Uuid;

pub struct SledDatabase {
    swaps: sled::Tree,
    peers: sled::Tree,
    addresses: sled::Tree,
    monero_addresses: sled::Tree,
}

impl SledDatabase {
    pub fn open(path: &Path) -> Result<Self> {
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

    pub async fn insert_peer_id(&self, swap_id: Uuid, peer_id: PeerId) -> Result<()> {
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

    pub fn get_peer_id(&self, swap_id: Uuid) -> Result<PeerId> {
        let key = serialize(&swap_id)?;

        let encoded = self
            .peers
            .get(&key)?
            .ok_or_else(|| anyhow!("No peer-id found for swap id {} in database", swap_id))?;

        let peer_id: String = deserialize(&encoded).context("Could not deserialize peer-id")?;
        Ok(PeerId::from_str(peer_id.as_str())?)
    }

    pub async fn insert_monero_address(
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

    pub fn get_monero_address(&self, swap_id: Uuid) -> Result<monero::Address> {
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

    pub async fn insert_address(&self, peer_id: PeerId, address: Multiaddr) -> Result<()> {
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

    pub fn get_addresses(&self, peer_id: PeerId) -> Result<Vec<Multiaddr>> {
        let key = peer_id.to_bytes();

        let addresses = match self.addresses.get(&key)? {
            Some(encoded) => deserialize(&encoded).context("Failed to deserialize addresses")?,
            None => vec![],
        };

        Ok(addresses)
    }

    pub async fn insert_latest_state(&self, swap_id: Uuid, state: Swap) -> Result<()> {
        let key = serialize(&swap_id)?;
        let new_value = serialize(&state).context("Could not serialize new state value")?;

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

    pub fn get_state(&self, swap_id: Uuid) -> Result<Swap> {
        let key = serialize(&swap_id)?;

        let encoded = self
            .swaps
            .get(&key)?
            .ok_or_else(|| anyhow!("Swap with id {} not found in database", swap_id))?;

        let state = deserialize(&encoded).context("Could not deserialize state")?;
        Ok(state)
    }

    pub fn all_alice(&self) -> Result<Vec<(Uuid, Alice)>> {
        self.all_alice_iter().collect()
    }

    fn all_alice_iter(&self) -> impl Iterator<Item = Result<(Uuid, Alice)>> {
        self.all_swaps_iter().map(|item| {
            let (swap_id, swap) = item?;
            Ok((swap_id, swap.try_into_alice()?))
        })
    }

    pub fn all_bob(&self) -> Result<Vec<(Uuid, Bob)>> {
        self.all_bob_iter().collect()
    }

    fn all_bob_iter(&self) -> impl Iterator<Item = Result<(Uuid, Bob)>> {
        self.all_swaps_iter().map(|item| {
            let (swap_id, swap) = item?;
            Ok((swap_id, swap.try_into_bob()?))
        })
    }

    fn all_swaps_iter(&self) -> impl Iterator<Item = Result<(Uuid, Swap)>> {
        self.swaps.iter().map(|item| {
            let (key, value) = item.context("Failed to retrieve swap from DB")?;

            let swap_id = deserialize::<Uuid>(&key)?;
            let swap = deserialize::<Swap>(&value).context("Failed to deserialize swap")?;

            Ok((swap_id, swap))
        })
    }

    pub fn unfinished_alice(&self) -> Result<Vec<(Uuid, Alice)>> {
        self.all_alice_iter()
            .filter_ok(|(_swap_id, alice)| !matches!(alice, Alice::Done(_)))
            .collect()
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
    use crate::database::alice::{Alice, AliceEndState};
    use crate::database::bob::{Bob, BobEndState};
    use crate::database::{NotAlice, NotBob};

    #[tokio::test]
    async fn can_write_and_read_to_multiple_keys() {
        let db_dir = tempfile::tempdir().unwrap();
        let db = SledDatabase::open(db_dir.path()).unwrap();

        let state_1 = Swap::Alice(Alice::Done(AliceEndState::BtcRedeemed));
        let swap_id_1 = Uuid::new_v4();
        db.insert_latest_state(swap_id_1, state_1.clone())
            .await
            .expect("Failed to save second state");

        let state_2 = Swap::Bob(Bob::Done(BobEndState::SafelyAborted));
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
        let db = SledDatabase::open(db_dir.path()).unwrap();

        let state = Swap::Alice(Alice::Done(AliceEndState::SafelyAborted));

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
    async fn all_swaps_as_alice() {
        let db_dir = tempfile::tempdir().unwrap();
        let db = SledDatabase::open(db_dir.path()).unwrap();

        let alice_state = Alice::Done(AliceEndState::BtcPunished);
        let alice_swap = Swap::Alice(alice_state.clone());
        let alice_swap_id = Uuid::new_v4();
        db.insert_latest_state(alice_swap_id, alice_swap)
            .await
            .expect("Failed to save alice state 1");

        let alice_swaps = db.all_alice().unwrap();
        assert_eq!(alice_swaps.len(), 1);
        assert!(alice_swaps.contains(&(alice_swap_id, alice_state)));

        let bob_state = Bob::Done(BobEndState::SafelyAborted);
        let bob_swap = Swap::Bob(bob_state);
        let bob_swap_id = Uuid::new_v4();
        db.insert_latest_state(bob_swap_id, bob_swap)
            .await
            .expect("Failed to save bob state 1");

        let err = db.all_alice().unwrap_err();

        assert_eq!(err.downcast_ref::<NotAlice>().unwrap(), &NotAlice);
    }

    #[tokio::test]
    async fn all_swaps_as_bob() {
        let db_dir = tempfile::tempdir().unwrap();
        let db = SledDatabase::open(db_dir.path()).unwrap();

        let bob_state = Bob::Done(BobEndState::SafelyAborted);
        let bob_swap = Swap::Bob(bob_state.clone());
        let bob_swap_id = Uuid::new_v4();
        db.insert_latest_state(bob_swap_id, bob_swap)
            .await
            .expect("Failed to save bob state 1");

        let bob_swaps = db.all_bob().unwrap();
        assert_eq!(bob_swaps.len(), 1);
        assert!(bob_swaps.contains(&(bob_swap_id, bob_state)));

        let alice_state = Alice::Done(AliceEndState::BtcPunished);
        let alice_swap = Swap::Alice(alice_state);
        let alice_swap_id = Uuid::new_v4();
        db.insert_latest_state(alice_swap_id, alice_swap)
            .await
            .expect("Failed to save alice state 1");

        let err = db.all_bob().unwrap_err();

        assert_eq!(err.downcast_ref::<NotBob>().unwrap(), &NotBob);
    }

    #[tokio::test]
    async fn can_save_swap_state_and_peer_id_with_same_swap_id() -> Result<()> {
        let db_dir = tempfile::tempdir().unwrap();
        let db = SledDatabase::open(db_dir.path()).unwrap();

        let alice_id = Uuid::new_v4();
        let alice_state = Alice::Done(AliceEndState::BtcPunished);
        let alice_swap = Swap::Alice(alice_state);
        let peer_id = PeerId::random();

        db.insert_latest_state(alice_id, alice_swap.clone()).await?;
        db.insert_peer_id(alice_id, peer_id).await?;

        let loaded_swap = db.get_state(alice_id)?;
        let loaded_peer_id = db.get_peer_id(alice_id)?;

        assert_eq!(alice_swap, loaded_swap);
        assert_eq!(peer_id, loaded_peer_id);

        Ok(())
    }

    #[tokio::test]
    async fn test_reopen_db() -> Result<()> {
        let db_dir = tempfile::tempdir().unwrap();
        let alice_id = Uuid::new_v4();
        let alice_state = Alice::Done(AliceEndState::BtcPunished);
        let alice_swap = Swap::Alice(alice_state);

        let peer_id = PeerId::random();

        {
            let db = SledDatabase::open(db_dir.path()).unwrap();
            db.insert_latest_state(alice_id, alice_swap.clone()).await?;
            db.insert_peer_id(alice_id, peer_id).await?;
        }

        let db = SledDatabase::open(db_dir.path()).unwrap();

        let loaded_swap = db.get_state(alice_id)?;
        let loaded_peer_id = db.get_peer_id(alice_id)?;

        assert_eq!(alice_swap, loaded_swap);
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
            let db = SledDatabase::open(db_dir.path())?;
            db.insert_address(peer_id, home1.clone()).await?;
            db.insert_address(peer_id, home2.clone()).await?;
        }

        let addresses = SledDatabase::open(db_dir.path())?.get_addresses(peer_id)?;

        assert_eq!(addresses, vec![home1, home2]);

        Ok(())
    }

    #[tokio::test]
    async fn save_and_load_monero_address() -> Result<()> {
        let db_dir = tempfile::tempdir()?;
        let swap_id = Uuid::new_v4();

        SledDatabase::open(db_dir.path())?.insert_monero_address(swap_id, "53gEuGZUhP9JMEBZoGaFNzhwEgiG7hwQdMCqFxiyiTeFPmkbt1mAoNybEUvYBKHcnrSgxnVWgZsTvRBaHBNXPa8tHiCU51a".parse()?).await?;
        let loaded_monero_address =
            SledDatabase::open(db_dir.path())?.get_monero_address(swap_id)?;

        assert_eq!(loaded_monero_address.to_string(), "53gEuGZUhP9JMEBZoGaFNzhwEgiG7hwQdMCqFxiyiTeFPmkbt1mAoNybEUvYBKHcnrSgxnVWgZsTvRBaHBNXPa8tHiCU51a");

        Ok(())
    }
}
