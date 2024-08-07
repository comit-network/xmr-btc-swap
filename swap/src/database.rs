pub use alice::Alice;
pub use bob::Bob;
pub use sqlite::SqliteDatabase;

use crate::fs::ensure_directory_exists;
use crate::protocol::{Database, State};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::path::Path;
use std::sync::Arc;

mod alice;
mod bob;
mod sqlite;

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

impl From<Swap> for State {
    fn from(value: Swap) -> Self {
        match value {
            Swap::Alice(alice) => State::Alice(alice.into()),
            Swap::Bob(bob) => State::Bob(bob.into()),
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

#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq)]
#[error("Not in the role of Alice")]
struct NotAlice;

#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq)]
#[error("Not in the role of Bob")]
struct NotBob;

impl Swap {
    pub fn try_into_alice(self) -> Result<Alice> {
        match self {
            Swap::Alice(alice) => Ok(alice),
            Swap::Bob(_) => bail!(NotAlice),
        }
    }

    pub fn try_into_bob(self) -> Result<Bob> {
        match self {
            Swap::Bob(bob) => Ok(bob),
            Swap::Alice(_) => bail!(NotBob),
        }
    }
}

pub async fn open_db(sqlite_path: impl AsRef<Path>) -> Result<Arc<dyn Database + Send + Sync>> {
    if sqlite_path.as_ref().exists() {
        tracing::debug!("Using existing sqlite database.");
        let sqlite = SqliteDatabase::open(sqlite_path).await?;
        Ok(Arc::new(sqlite))
    } else {
        tracing::debug!("Creating and using new sqlite database.");
        ensure_directory_exists(sqlite_path.as_ref())?;
        tokio::fs::File::create(&sqlite_path).await?;
        let sqlite = SqliteDatabase::open(sqlite_path).await?;
        Ok(Arc::new(sqlite))
    }
}
