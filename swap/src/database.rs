use crate::database::alice::Alice;
use crate::database::bob::Bob;
use crate::protocol::alice::AliceState;
use crate::protocol::bob::BobState;
use crate::protocol::{State, Database};
use std::fmt::Display;
use serde::{Deserialize, Serialize};
use anyhow::bail;
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use std::collections::HashSet;

mod sled;
mod sqlite;
mod alice;
mod bob;

pub use self::sled::SledDatabase;
pub use sqlite::SqliteDatabase;

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

pub async fn open_db(sled_path: impl AsRef<Path>, sqlite_path: impl AsRef<Path>, sled_flag: bool) -> Result<Arc<dyn Database + Send + Sync>> {
    // if sled exists and sqlite doesnt exist try and migrate
    // if sled and sqlite exists and the sled flag is set, use sled
    // if sled and sqlite exists, use sqlite
    match (sled_path.as_ref().exists(), sqlite_path.as_ref().exists(), sled_flag) {
        (true, false, false) => {
            let sled_db = SledDatabase::open(sled_path.as_ref()).await?;
            let sqlite = SqliteDatabase::open(sqlite_path).await?;

            let swaps = sled_db.all().await?;

            for (swap_id, state) in swaps.iter() {
                sqlite.insert_latest_state(*swap_id, state.clone()).await?;
                let monero_address = sled_db.get_monero_address(*swap_id).await?;
                sqlite.insert_monero_address(*swap_id, monero_address).await?;
            }

            let mut unique_peer_ids = HashSet::new();

            for (swap_id, _) in swaps.iter() {
                let peer_id = sled_db.get_peer_id(*swap_id).await?;
                sqlite.insert_peer_id(*swap_id, peer_id).await?;
                unique_peer_ids.insert(peer_id);
            }

            for peer_id in unique_peer_ids {
                let addresses_of_peer = sled_db.get_addresses(peer_id).await?;

                for address in addresses_of_peer.iter() {
                    sqlite.insert_address(peer_id, address.clone()).await?;
                }
            }

            Ok(Arc::new(sqlite))
        }
        (_, _, false) => {
            let sqlite = SqliteDatabase::open(sqlite_path).await?;
            Ok(Arc::new(sqlite))
        }
        (false, _, true) => {
            bail!("Sled database does not exist at specified location")
        }
        (true, _, true) => {
            let sled= SledDatabase::open(sled_path.as_ref()).await?;
            Ok(Arc::new(sled))
        }
    }

}
