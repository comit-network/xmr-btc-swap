pub mod peer_tracker;
pub mod request_response;
pub mod transport;

use crate::seed::SEED_LENGTH;
use bitcoin::hashes::{sha256, Hash, HashEngine};
use futures::prelude::*;
use libp2p::{core::Executor, identity::ed25519};
use std::pin::Pin;
use tokio::runtime::Handle;

#[allow(missing_debug_implementations)]
pub struct TokioExecutor {
    pub handle: Handle,
}

impl Executor for TokioExecutor {
    fn exec(&self, future: Pin<Box<dyn Future<Output = ()> + Send>>) {
        let _ = self.handle.spawn(future);
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct Seed([u8; SEED_LENGTH]);

impl Seed {
    /// prefix "NETWORK" to the provided seed and apply sha256
    pub fn new(seed: crate::seed::Seed) -> Self {
        let mut engine = sha256::HashEngine::default();

        engine.input(&seed.bytes());
        engine.input(b"NETWORK");

        let hash = sha256::Hash::from_engine(engine);
        Self(hash.into_inner())
    }

    fn bytes(&self) -> [u8; SEED_LENGTH] {
        self.0
    }

    pub fn derive_libp2p_identity(&self) -> libp2p::identity::Keypair {
        let mut engine = sha256::HashEngine::default();

        engine.input(&self.bytes());
        engine.input(b"LIBP2P_IDENTITY");

        let hash = sha256::Hash::from_engine(engine);
        let key =
            ed25519::SecretKey::from_bytes(hash.into_inner()).expect("we always pass 32 bytes");
        libp2p::identity::Keypair::Ed25519(key.into())
    }
}
