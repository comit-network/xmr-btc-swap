use crate::testutils::init_wallets;
use anyhow::Result;
use bitcoin_harness::Bitcoind;
use futures::future::{select, Select};
use libp2p::{core::Multiaddr, PeerId};
use monero_harness::Monero;
use rand::rngs::OsRng;
use std::{pin::Pin, sync::Arc};
use swap::{
    bitcoin,
    config::Config,
    database::Database,
    monero, network,
    network::transport::build,
    protocol::{bob, bob::BobState},
    seed::Seed,
    SwapAmounts,
};
use tempfile::tempdir;
use uuid::Uuid;

pub struct Bob {
    state: BobState,
    event_loop: bob::event_loop::EventLoop,
    event_loop_handle: bob::event_loop::EventLoopHandle,
    bitcoin_wallet: Arc<swap::bitcoin::Wallet>,
    monero_wallet: Arc<swap::monero::Wallet>,
    db: Database,
}

impl Bob {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        alice_multiaddr: Multiaddr,
        alice_peer_id: PeerId,
        bitcoind: &Bitcoind<'_>,
        monero: &Monero,
        btc_to_swap: bitcoin::Amount,
        xmr_to_swap: monero::Amount,
        btc_starting_balance: bitcoin::Amount,
        config: Config,
    ) -> Bob {
        let (bob_btc_wallet, bob_xmr_wallet) = init_wallets(
            "bob",
            bitcoind,
            monero,
            Some(btc_starting_balance),
            None,
            config,
        )
        .await;

        let bob_state =
            init_bob_state(btc_to_swap, xmr_to_swap, bob_btc_wallet.clone(), config).await;

        let (event_loop, event_loop_handle) = init_bob_event_loop(alice_peer_id, alice_multiaddr);

        let bob_db_dir = tempdir().unwrap();
        let bob_db = Database::open(bob_db_dir.path()).unwrap();

        Bob {
            state: bob_state,
            event_loop,
            event_loop_handle,
            bitcoin_wallet: bob_btc_wallet,
            monero_wallet: bob_xmr_wallet,
            db: bob_db,
        }
    }
    pub async fn swap(
        &self,
    ) -> Select<Pin<Box<Result<BobState>>>, Pin<Box<Result<bob::EventLoop>>>> {
        let bob_swap_fut = bob::swap::swap(
            self.state.clone(),
            self.event_loop_handle,
            self.db,
            self.bitcoin_wallet,
            self.monero_wallet,
            OsRng,
            Uuid::new_v4(),
        );

        let bob_fut = select(Box::pin(bob_swap_fut), Box::pin(self.event_loop.run()));
        bob_fut
    }

    pub async fn assert_btc_redeemed(&self) {}
}

pub async fn init_bob_state(
    btc_to_swap: bitcoin::Amount,
    xmr_to_swap: monero::Amount,
    bob_btc_wallet: Arc<bitcoin::Wallet>,
    config: Config,
) -> BobState {
    let amounts = SwapAmounts {
        btc: btc_to_swap,
        xmr: xmr_to_swap,
    };

    let refund_address = bob_btc_wallet.new_address().await.unwrap();
    let state0 = bob::State0::new(
        &mut OsRng,
        btc_to_swap,
        xmr_to_swap,
        config.bitcoin_cancel_timelock,
        config.bitcoin_punish_timelock,
        refund_address,
        config.monero_finality_confirmations,
    );

    BobState::Started { state0, amounts }
}

pub fn init_bob_event_loop(
    alice_peer_id: PeerId,
    alice_addr: Multiaddr,
) -> (bob::event_loop::EventLoop, bob::event_loop::EventLoopHandle) {
    let seed = Seed::random().unwrap();
    let bob_behaviour = bob::Behaviour::new(network::Seed::new(seed));
    let bob_transport = build(bob_behaviour.identity()).unwrap();
    bob::event_loop::EventLoop::new(bob_transport, bob_behaviour, alice_peer_id, alice_addr)
        .unwrap()
}
