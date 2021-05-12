pub mod harness;

use rand::rngs::OsRng;
use swap::bitcoin::TxLock;
use swap::env::GetConfig;
use swap::monero;
use swap::protocol::alice::event_loop::FixedRate;
use swap::protocol::CROSS_CURVE_PROOF_SYSTEM;
use swap::seed::Seed;
use swap::xmr_first_protocol::alice::{publish_xmr_refund, Alice3};
use swap::xmr_first_protocol::bob::Bob3;
use tempfile::tempdir;
use testcontainers::clients::Cli;

#[tokio::test]
async fn refund() {
    let cli = Cli::default();

    let env_config = harness::SlowCancelConfig::get_config();

    let (monero, containers) = harness::init_containers(&cli).await;

    let btc_swap_amount = bitcoin::Amount::from_sat(1_000_000);
    let xmr_swap_amount =
        monero::Amount::from_monero(btc_swap_amount.as_btc() / FixedRate::RATE).unwrap();

    let alice_starting_balances = harness::StartingBalances {
        xmr: xmr_swap_amount * 10,
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
        btc: btc_swap_amount * 10,
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

    let a = swap::bitcoin::SecretKey::new_random(&mut OsRng);
    let b = swap::bitcoin::SecretKey::new_random(&mut OsRng);

    let s_a = monero::Scalar::random(&mut OsRng);
    let S_a = monero::PublicKey::from_private_key(&monero::PrivateKey { scalar: s_a });

    let s_b = monero::Scalar::random(&mut OsRng);
    let S_b = monero::PublicKey::from_private_key(&monero::PrivateKey { scalar: s_b });

    let (dleq_proof_s_b, (S_b_bitcoin, S_b_monero)) =
        CROSS_CURVE_PROOF_SYSTEM.prove(&s_b, &mut OsRng);

    let v_a = monero::PrivateViewKey::new_random(&mut OsRng);
    let v_b = monero::PrivateViewKey::new_random(&mut OsRng);

    let tx_lock = TxLock::new(&bob_bitcoin_wallet, btc_swap_amount, a.public(), b.public()).await?;

    let alice = Alice3 {
        xmr_swap_amount,
        btc_swap_amount,
        a,
        B: b.public(),
        s_a,
        S_b_monero: monero::PublicKey {
            point: S_b_monero.compress(),
        },
        v_a,
        redeem_address: alice_bitcoin_wallet.new_address().await?,
    };

    let bob = Bob3 {
        xmr_swap_amount,
        btc_swap_amount,
        tx_lock,
        S: S_b,
        v_b,
        alice_redeem_address: bob_bitcoin_wallet.new_address().await?,
    };

    let alice = alice.publish_xmr_lock(&alice_monero_wallet).await.unwrap();

    publish_xmr_refund(&alice_bitcoin_wallet).await.unwrap();
}
