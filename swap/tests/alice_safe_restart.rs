use bitcoin_harness::Bitcoind;
use libp2p::Multiaddr;
use monero_harness::Monero;
use rand::rngs::OsRng;
use swap::{alice, alice::swap::AliceState, bitcoin, bob, storage::Database};
use tempfile::tempdir;
use testcontainers::clients::Cli;
use uuid::Uuid;
use xmr_btc::config::Config;

pub mod testutils;

use crate::testutils::{init_alice, init_bob};
use std::convert::TryFrom;
use testutils::init_tracing;

#[tokio::test]
async fn given_alice_restarts_after_encsig_is_learned_resume_swap() {
    let _guard = init_tracing();

    let cli = Cli::default();
    let bitcoind = Bitcoind::new(&cli, "0.19.1").unwrap();
    let _ = bitcoind.init(5).await;
    let (monero, _container) =
        Monero::new(&cli, None, vec!["alice".to_string(), "bob".to_string()])
            .await
            .unwrap();

    let btc_to_swap = bitcoin::Amount::from_sat(1_000_000);
    let xmr_to_swap = xmr_btc::monero::Amount::from_piconero(1_000_000_000_000);

    let bob_btc_starting_balance = btc_to_swap * 10;
    let bob_xmr_starting_balance = xmr_btc::monero::Amount::ZERO;

    let alice_btc_starting_balance = bitcoin::Amount::ZERO;
    let alice_xmr_starting_balance = xmr_to_swap * 10;

    let alice_multiaddr: Multiaddr = "/ip4/127.0.0.1/tcp/9877"
        .parse()
        .expect("failed to parse Alice's address");

    let config = Config::regtest();

    let (
        alice_state,
        mut alice_event_loop,
        alice_event_loop_handle,
        alice_btc_wallet,
        alice_xmr_wallet,
    ) = init_alice(
        &bitcoind,
        &monero,
        btc_to_swap,
        alice_btc_starting_balance,
        xmr_to_swap,
        alice_xmr_starting_balance,
        alice_multiaddr.clone(),
        config,
    )
    .await;

    let (bob_state, bob_event_loop, bob_event_loop_handle, bob_btc_wallet, bob_xmr_wallet, bob_db) =
        init_bob(
            alice_multiaddr,
            &bitcoind,
            &monero,
            btc_to_swap,
            bob_btc_starting_balance,
            xmr_to_swap,
            bob_xmr_starting_balance,
            config,
        )
        .await;

    let _ = tokio::spawn(async move {
        bob::swap::swap(
            bob_state,
            bob_event_loop_handle,
            bob_db,
            bob_btc_wallet.clone(),
            bob_xmr_wallet.clone(),
            OsRng,
            Uuid::new_v4(),
        )
        .await
    });

    let _bob_swarm_fut = tokio::spawn(async move { bob_event_loop.run().await });

    let alice_db_datadir = tempdir().unwrap();
    let alice_db = Database::open(alice_db_datadir.path()).unwrap();

    let _alice_swarm_fut = tokio::spawn(async move { alice_event_loop.run().await });

    let alice_swap_id = Uuid::new_v4();

    let (alice_state, alice_event_loop_handle) = alice::swap::run_until(
        alice_state,
        alice::swap::is_encsig_learned,
        alice_event_loop_handle,
        alice_btc_wallet.clone(),
        alice_xmr_wallet.clone(),
        Config::regtest(),
        alice_swap_id,
        alice_db,
    )
    .await
    .unwrap();

    assert!(matches!(alice_state, AliceState::EncSignLearned {..}));

    // todo: add db code here
    let alice_db = Database::open(alice_db_datadir.path()).unwrap();
    let alice_state = alice_db.get_state(alice_swap_id).unwrap();

    let (alice_state, _) = alice::swap::swap(
        AliceState::try_from(alice_state).unwrap(),
        alice_event_loop_handle,
        alice_btc_wallet.clone(),
        alice_xmr_wallet.clone(),
        Config::regtest(),
        Uuid::new_v4(),
        alice_db,
    )
    .await
    .unwrap();

    assert!(matches!(alice_state, AliceState::BtcRedeemed {..}));
}
// #[tokio::test]
// async fn given_alice_restarts_after_xmr_is_locked_refund_swap() {
//     setup_tracing();
//
//     let config = Config::regtest();
//
//     let alice_multiaddr: Multiaddr = "/ip4/127.0.0.1/tcp/9876"
//         .parse()
//         .expect("failed to parse Alice's address");
//
//     let btc_to_swap = bitcoin::Amount::from_sat(1_000_000);
//     let init_btc_alice = bitcoin::Amount::ZERO;
//     let init_btc_bob = btc_to_swap * 10;
//
//     let xmr_to_swap = monero::Amount::from_piconero(1_000_000_000_000);
//     let init_xmr_alice = xmr_to_swap * 10;
//     let init_xmr_bob = monero::Amount::ZERO;
//
//     let cli = Cli::default();
//     let (alice_btc_wallet, alice_xmr_wallet, bob_btc_wallet, bob_xmr_wallet,
// _containers) =         setup_wallets(
//             &cli,
//             init_btc_alice,
//             init_xmr_alice,
//             init_btc_bob,
//             init_xmr_bob,
//             config
//         )
//             .await;
//
//     let alice_btc_wallet = Arc::new(alice_btc_wallet);
//     let alice_xmr_wallet = Arc::new(alice_xmr_wallet);
//     let bob_btc_wallet = Arc::new(bob_btc_wallet);
//     let bob_xmr_wallet = Arc::new(bob_xmr_wallet);
//
//     let amounts = SwapAmounts {
//         btc: btc_to_swap,
//         xmr: xmr_to_swap,
//     };
//
//     let alice_db_dir = TempDir::new().unwrap();
//     let alice_swap_fut = async {
//         let rng = &mut OsRng;
//         let (alice_start_state, state0) = {
//             let a = bitcoin::SecretKey::new_random(rng);
//             let s_a = cross_curve_dleq::Scalar::random(rng);
//             let v_a = xmr_btc::monero::PrivateViewKey::new_random(rng);
//             let redeem_address =
// alice_btc_wallet.as_ref().new_address().await.unwrap();             let
// punish_address = redeem_address.clone();             let state0 =
// xmr_btc::alice::State0::new(                 a,
//                 s_a,
//                 v_a,
//                 amounts.btc,
//                 amounts.xmr,
//                 config.bitcoin_refund_timelock,
//                 config.bitcoin_punish_timelock,
//                 redeem_address,
//                 punish_address,
//             );
//
//             (
//                 AliceState::Started {
//                     amounts,
//                     state0: state0.clone(),
//                 },
//                 state0,
//             )
//         };
//         let alice_behaviour = alice::Behaviour::new(state0.clone());
//         let alice_transport = build(alice_behaviour.identity()).unwrap();
//         let (mut alice_event_loop_1, alice_event_loop_handle) =
// alice::event_loop::EventLoop::new(             alice_transport,
//             alice_behaviour,
//             alice_multiaddr.clone(),
//         )
//             .unwrap();
//
//         let _alice_event_loop_1 = tokio::spawn(async move {
// alice_event_loop_1.run().await });
//
//         let config = xmr_btc::config::Config::regtest();
//         let swap_id = Uuid::new_v4();
//
//         let db = Database::open(alice_db_dir.path()).unwrap();
//
//         // Alice reaches encsig_learned
//         alice::swap::run_until(
//             alice_start_state,
//             |state| matches!(state, AliceState::XmrLocked { .. }),
//             alice_event_loop_handle,
//             alice_btc_wallet.clone(),
//             alice_xmr_wallet.clone(),
//             config,
//             swap_id,
//             db,
//         )
//             .await
//             .unwrap();
//
//         let db = Database::open(alice_db_dir.path()).unwrap();
//
//         let alice_behaviour = alice::Behaviour::new(state0);
//         let alice_transport = build(alice_behaviour.identity()).unwrap();
//         let (mut alice_event_loop_2, alice_event_loop_handle) =
// alice::event_loop::EventLoop::new(             alice_transport,
//             alice_behaviour,
//             alice_multiaddr.clone(),
//         )
//             .unwrap();
//
//         let _alice_event_loop_2 = tokio::spawn(async move {
// alice_event_loop_2.run().await });
//
//         // Load the latest state from the db
//         let latest_state = db.get_state(swap_id).unwrap();
//         let latest_state = latest_state.try_into().unwrap();
//
//         // Finish the swap
//         alice::swap::swap(
//             latest_state,
//             alice_event_loop_handle,
//             alice_btc_wallet.clone(),
//             alice_xmr_wallet.clone(),
//             config,
//             swap_id,
//             db,
//         )
//             .await
//     };
//
//     let (bob_swap, bob_event_loop) = {
//         let rng = &mut OsRng;
//         let bob_db_dir = tempdir().unwrap();
//         let bob_db = Database::open(bob_db_dir.path()).unwrap();
//         let bob_behaviour = bob::Behaviour::default();
//         let bob_transport = build(bob_behaviour.identity()).unwrap();
//
//         let refund_address = bob_btc_wallet.new_address().await.unwrap();
//         let state0 = xmr_btc::bob::State0::new(
//             rng,
//             btc_to_swap,
//             xmr_to_swap,
//             config.bitcoin_refund_timelock,
//             config.bitcoin_punish_timelock,
//             refund_address,
//         );
//         let bob_state = BobState::Started {
//             state0,
//             amounts,
//             addr: alice_multiaddr.clone(),
//         };
//         let (bob_event_loop, bob_event_loop_handle) =
//             bob::event_loop::EventLoop::new(bob_transport,
// bob_behaviour).unwrap();
//
//         (
//             bob::swap::swap(
//                 bob_state,
//                 bob_event_loop_handle,
//                 bob_db,
//                 bob_btc_wallet.clone(),
//                 bob_xmr_wallet.clone(),
//                 OsRng,
//                 Uuid::new_v4(),
//             ),
//             bob_event_loop,
//         )
//     };
//
//     let _bob_event_loop = tokio::spawn(async move {
// bob_event_loop.run().await });
//
//     try_join(alice_swap_fut, bob_swap).await.unwrap();
//
//     let btc_alice_final = alice_btc_wallet.balance().await.unwrap();
//     let xmr_alice_final = alice_xmr_wallet.get_balance().await.unwrap();
//
//     let btc_bob_final = bob_btc_wallet.balance().await.unwrap();
//     bob_xmr_wallet.0.refresh().await.unwrap();
//     let xmr_bob_final = bob_xmr_wallet.get_balance().await.unwrap();
//
//     // Alice's BTC balance did not change
//     assert_eq!(btc_alice_final, init_btc_alice);
//     // Bob wasted some BTC fees
//     assert_eq!(
//         btc_bob_final,
//         init_btc_bob - bitcoin::Amount::from_sat(bitcoin::TX_FEE)
//     );
//
//     // Alice wasted some XMR fees
//     assert_eq!(init_xmr_alice - xmr_alice_final, monero::Amount::ZERO);
//     // Bob's ZMR balance did not change
//     assert_eq!(xmr_bob_final, init_xmr_bob);
// }
