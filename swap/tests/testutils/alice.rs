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
        alice,
        alice::{swap::AliceActor, AliceState, EventLoop},
    },
    seed::Seed,
    SwapAmounts,
};
use tempfile::tempdir;
use tokio::select;
use uuid::Uuid;

pub struct Alice {
    state: AliceState,
    actor: AliceActor,
    event_loop: EventLoop,
}

impl Alice {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        bitcoind: &Bitcoind<'_>,
        monero: &Monero,
        btc_to_swap: bitcoin::Amount,
        xmr_to_swap: monero::Amount,
        xmr_starting_balance: monero::Amount,
        listen: Multiaddr,
        config: Config,
        seed: Seed,
    ) -> Alice {
        let (alice_btc_wallet, alice_xmr_wallet) = init_wallets(
            "alice",
            bitcoind,
            monero,
            None,
            Some(xmr_starting_balance),
            config,
        )
        .await;

        let alice_start_state =
            init_alice_state(btc_to_swap, xmr_to_swap, alice_btc_wallet.clone(), config).await;

        let (event_loop, event_loop_handle) = init_alice_event_loop(listen, seed);

        let alice_db_datadir = tempdir().unwrap();
        let alice_db = Database::open(alice_db_datadir.path()).unwrap();
        let alice_actor = AliceActor::new(
            event_loop_handle,
            alice_btc_wallet,
            alice_xmr_wallet,
            alice_db,
            config,
            Uuid::new_v4(),
        );

        Alice {
            state: alice_start_state,
            actor: alice_actor,
            event_loop,
        }
    }

    pub fn peer_id(&self) -> PeerId {
        self.event_loop.peer_id()
    }

    pub async fn swap(mut self) -> Result<()> {
        let final_state = select! {
            res = self.actor.swap(self.state) => res.unwrap(),
            _ = self.event_loop.run() => panic!("The event loop should never finish")
        };
        self.state = final_state;
        Ok(())
    }

    pub async fn assert_btc_redeemed(&self) {}
}

pub async fn init_alice_state(
    btc_to_swap: bitcoin::Amount,
    xmr_to_swap: monero::Amount,
    alice_btc_wallet: Arc<bitcoin::Wallet>,
    config: Config,
) -> AliceState {
    let rng = &mut OsRng;

    let amounts = SwapAmounts {
        btc: btc_to_swap,
        xmr: xmr_to_swap,
    };

    let a = bitcoin::SecretKey::new_random(rng);
    let s_a = cross_curve_dleq::Scalar::random(rng);
    let v_a = monero::PrivateViewKey::new_random(rng);
    let redeem_address = alice_btc_wallet.as_ref().new_address().await.unwrap();
    let punish_address = redeem_address.clone();
    let state0 = alice::State0::new(
        a,
        s_a,
        v_a,
        amounts.btc,
        amounts.xmr,
        config.bitcoin_cancel_timelock,
        config.bitcoin_punish_timelock,
        redeem_address,
        punish_address,
    );

    AliceState::Started { amounts, state0 }
}

pub fn init_alice_event_loop(
    listen: Multiaddr,
    seed: Seed,
) -> (
    alice::event_loop::EventLoop,
    alice::event_loop::EventLoopHandle,
) {
    let alice_behaviour = alice::Behaviour::new(network::Seed::new(seed));
    let alice_transport = build(alice_behaviour.identity()).unwrap();
    alice::event_loop::EventLoop::new(alice_transport, alice_behaviour, listen).unwrap()
}
