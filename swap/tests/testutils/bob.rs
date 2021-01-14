use crate::testutils::init_wallets;
use anyhow::Result;
use bitcoin_harness::Bitcoind;
use libp2p::{core::Multiaddr, PeerId};
use monero_harness::Monero;
use rand::rngs::OsRng;
use std::sync::Arc;
use swap::{
    bitcoin,
    config::Config,
    database::Database,
    monero, network,
    network::transport::build,
    protocol::{
        bob,
        bob::{swap::BobActor, BobState, EventLoop},
    },
    seed::Seed,
    SwapAmounts,
};
use tempfile::tempdir;
use tokio::select;
use uuid::Uuid;

pub struct Bob {
    actor: BobActor,
    event_loop: EventLoop,
    final_state: Option<BobState>,
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

        let (event_loop, event_loop_handle) = init_bob_event_loop(alice_peer_id, alice_multiaddr);

        let bob_db_dir = tempdir().unwrap();
        let bob_db = Database::open(bob_db_dir.path()).unwrap();

        let bob_actor = BobActor::new(event_loop_handle, bob_btc_wallet, bob_xmr_wallet, bob_db);

        let bob_state =
            init_bob_state(btc_to_swap, xmr_to_swap, bob_btc_wallet.clone(), config).await;

        Bob {
            final_state: Some(bob_state),
            actor: bob_actor,
            event_loop,
        }
    }
    pub async fn swap(&mut self) -> Result<()> {
        let final_state = select! {
            res = self.actor.swap(bob_state, Uuid::new_v4()) => res.unwrap(),
            _ = self.event_loop.run() => panic!("The event loop should never finish")
        };
        self.final_state = Some(final_state);
        Ok(())
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
