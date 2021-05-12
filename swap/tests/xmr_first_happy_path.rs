pub mod harness;

use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
use curve25519_dalek::edwards::EdwardsPoint;
use monero_adaptor::alice::Alice0;
use monero_adaptor::bob::Bob0;
use rand::rngs::OsRng;
use swap::env::GetConfig;
use swap::monero;
use swap::monero::{PublicKey, Scalar};
use swap::protocol::alice::event_loop::FixedRate;
use swap::protocol::CROSS_CURVE_PROOF_SYSTEM;
use swap::seed::Seed;
use swap::xmr_first_protocol::alice::Alice3;
use swap::xmr_first_protocol::bob::Bob3;
use swap::xmr_first_protocol::{alice, bob};
use tempfile::tempdir;
use testcontainers::clients::Cli;

#[tokio::test]
async fn happy_path() {
    let cli = Cli::default();

    let env_config = harness::SlowCancelConfig::get_config();

    let (monero, containers) = harness::init_containers(&cli).await;

    let btc_amount = bitcoin::Amount::from_sat(1_000_000);
    let xmr_amount = monero::Amount::from_monero(btc_amount.as_btc() / FixedRate::RATE).unwrap();

    let alice_starting_balances = harness::StartingBalances {
        xmr: xmr_amount * 10,
        btc: bitcoin::Amount::ZERO,
    };

    let electrs_rpc_port = containers
        .electrs
        .get_host_port(harness::electrs::RPC_PORT)
        .expect("Could not map electrs rpc port");

    let alice_seed = Seed::random().unwrap();
    let (alice_bitcoin_wallet, alice_monero_wallet) = harness::init_test_wallets(
        "Alice",
        containers.bitcoind_url.clone(),
        &monero,
        alice_starting_balances.clone(),
        tempdir().unwrap().path(),
        electrs_rpc_port,
        &alice_seed,
        env_config.clone(),
    )
    .await;

    let bob_seed = Seed::random().unwrap();
    let bob_starting_balances = harness::StartingBalances {
        xmr: monero::Amount::ZERO,
        btc: btc_amount * 10,
    };

    let (bob_bitcoin_wallet, bob_monero_wallet) = harness::init_test_wallets(
        "Bob",
        containers.bitcoind_url,
        &monero,
        bob_starting_balances.clone(),
        tempdir().unwrap().path(),
        electrs_rpc_port,
        &bob_seed,
        env_config,
    )
    .await;

    let a = crate::bitcoin::SecretKey::new_random(rng);
    let b = crate::bitcoin::SecretKey::new_random(rng);

    let s_a = monero::Scalar::random(rng);
    let s_b = monero::Scalar::random(rng);

    let (dleq_proof_s_b, (S_b_bitcoin, S_b_monero)) = CROSS_CURVE_PROOF_SYSTEM.prove(&s_b, rng);

    let v_a = monero::PrivateViewKey::new_random(rng);
    let v_b = monero::PrivateViewKey::new_random(rng);

    let alice = Alice3 {
        xmr_swap_amount: xmr_amount,
        btc_swap_amount: btc_amount,
        a,
        B: b.public(),
        s_a,
        S_b_monero,
        v_a,
    };

    let bob = Bob3 {
        xmr_swap_amount,
        btc_swap_amount,
        xmr_lock,
        v_b,
    };

    alice.publish_xmr_lock(&alice_monero_wallet).await.unwrap();

    bob.watch_for_lock_xmr(&bob_monero_wallet_wallet)
        .await
        .unwrap();

    alice.publish_btc_redeem(&alice_btc_wallet).await.unwrap();

    bob.publish_xmr_redeem(&alice_monero_wallet).await.unwrap();
}
