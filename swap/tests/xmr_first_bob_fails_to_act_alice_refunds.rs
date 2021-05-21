pub mod harness;

use rand::rngs::OsRng;
use swap::bitcoin::BtcLock;
use swap::env::GetConfig;
use swap::monero;
use swap::monero::TransferRequest;
use swap::protocol::alice::event_loop::FixedRate;
use swap::protocol::CROSS_CURVE_PROOF_SYSTEM;
use swap::seed::Seed;
use swap::xmr_first_protocol::alice::{publish_xmr_refund, Alice3};
use swap::xmr_first_protocol::bob::Bob3;
use swap::xmr_first_protocol::transactions::btc_lock::BtcLock;
use swap::xmr_first_protocol::transactions::xmr_lock::XmrLock;
use swap::xmr_first_protocol::transactions::xmr_refund::XmrRefund;
use tempfile::tempdir;
use testcontainers::clients::Cli;
use swap::xmr_first_protocol::transactions::btc_redeem::BtcRedeem;
use monero::{PublicKey, PrivateKey};
use swap::xmr_first_protocol::setup;
use swap::xmr_first_protocol::transactions::xmr_redeem::XmrRedeem;

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

    let (alice, bob) = setup();

    let btc_redeem_address = alice_bitcoin_wallet.new_address().await.unwrap();

    // transactions
    let btc_lock =
        BtcLock::new(&bob_bitcoin_wallet, btc_swap_amount, a.public(), b.public()).await?;
    let btc_redeem = BtcRedeem::new(&btc_lock, &btc_redeem_address);
    let xmr_lock = XmrLock::new(alice.S_a.into(), alice.S_b, alice.v_a, alice.v_b, xmr_swap_amount);
    //let xmr_redeem = XmrRedeem::new(s_a, PrivateKey::from_scalar(bob.s_b), alice.v_a, alice.v_b, xmr_swap_amount);
    let xmr_refund = XmrRefund::new(sig, xmr_swap_amount);

    // Alice publishes xmr_lock
    let xmr_lock_transfer_proof = alice_monero_wallet
        .transfer(xmr_lock.transfer_request())
        .await
        .unwrap();

    // Bob waits until xmr_lock is seen
    let _ = bob_monero_wallet
        .watch_for_transfer(xmr_lock.watch_request(xmr_lock_transfer_proof))
        .await
        .unwrap();

    // Bob publishes btc_lock
    let signed_tx_lock = bob_bitcoin_wallet
        .sign_and_finalize(btc_lock.clone().into())
        .await?;
    let (_txid, sub) = bob_bitcoin_wallet.broadcast(signed_tx_lock, "lock").await.unwrap();
    let _ = sub.wait_until_confirmed_with(1).await?;

    // alice publishes xmr_refund
    // let xmr_refund_transfer_proof = alice_monero_wallet
    //     .transfer(xmr_refund.transfer_request())
    //     .await
    //     .unwrap();

    // alice publishes btc_redeem
    btc_redeem.encsig((), ());
    let (_, btc_redeem_sub) = alice_bitcoin_wallet.broadcast(btc_redeem.build_transaction(alice.a, alice.s_a, alice.pk_b, btc_lock.), "redeem")
        .await
        .unwrap();

    // bob sees xmr_refund and btc_redeem
    let _ = bob_monero_wallet
        .watch_for_transfer(xmr_lock.watch_request(xmr_refund_transfer_proof))
        .await
        .unwrap();
    let _ = btc_redeem_sub.wait_until_seen()
        .await
        .unwrap();

    // extract r_a from xmr_refund
    let _ = bob_bitcoin_wallet.broadcast("redeem")
}
