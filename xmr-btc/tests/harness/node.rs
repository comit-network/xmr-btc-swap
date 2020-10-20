use crate::harness::{transport::Transport, wallet};
use anyhow::Result;
use rand::{CryptoRng, RngCore};
use xmr_btc::{alice, bob};

// TODO: merge this with bob node
// This struct is responsible for I/O
pub struct AliceNode {
    transport: Transport<alice::Message, bob::Message>,
    pub bitcoin_wallet: wallet::bitcoin::Wallet,
    pub monero_wallet: wallet::monero::Wallet,
}

impl AliceNode {
    pub fn new(
        transport: Transport<alice::Message, bob::Message>,
        bitcoin_wallet: wallet::bitcoin::Wallet,
        monero_wallet: wallet::monero::Wallet,
    ) -> AliceNode {
        Self {
            transport,
            bitcoin_wallet,
            monero_wallet,
        }
    }
}

pub async fn run_alice_until<R: RngCore + CryptoRng>(
    alice: &mut AliceNode,
    initial_state: alice::State,
    is_state: fn(&alice::State) -> bool,
    rng: &mut R,
) -> Result<alice::State> {
    let mut result = initial_state;
    loop {
        result = alice::next_state(
            &alice.bitcoin_wallet,
            &alice.monero_wallet,
            &mut alice.transport,
            result,
            rng,
        )
        .await?;
        if is_state(&result) {
            return Ok(result);
        }
    }
}

// TODO: merge this with alice node
// This struct is responsible for I/O
pub struct BobNode {
    transport: Transport<bob::Message, alice::Message>,
    pub bitcoin_wallet: wallet::bitcoin::Wallet,
    pub monero_wallet: wallet::monero::Wallet,
}

impl BobNode {
    pub fn new(
        transport: Transport<bob::Message, alice::Message>,
        bitcoin_wallet: wallet::bitcoin::Wallet,
        monero_wallet: wallet::monero::Wallet,
    ) -> BobNode {
        Self {
            transport,
            bitcoin_wallet,
            monero_wallet,
        }
    }
}

pub async fn run_bob_until<R: RngCore + CryptoRng>(
    bob: &mut BobNode,
    initial_state: bob::State,
    is_state: fn(&bob::State) -> bool,
    rng: &mut R,
) -> Result<bob::State> {
    let mut result = initial_state;
    loop {
        result = bob::next_state(
            &bob.bitcoin_wallet,
            &bob.monero_wallet,
            &mut bob.transport,
            result,
            rng,
        )
        .await?;
        if is_state(&result) {
            return Ok(result);
        }
    }
}
