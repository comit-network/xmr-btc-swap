use bitcoin_harness::Bitcoind;
use futures::{channel::mpsc, future::try_join};
use libp2p::Multiaddr;
use monero_harness::Monero;
use std::sync::Arc;
use swap::{alice, bob};
use testcontainers::clients::Cli;
use tracing_subscriber::util::SubscriberInitExt;

#[tokio::test]
async fn swap() {
    let _guard = tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_ansi(false)
        .set_default();

    let alice_multiaddr: Multiaddr = "/ip4/127.0.0.1/tcp/9876"
        .parse()
        .expect("failed to parse Alice's address");

    let cli = Cli::default();
    let bitcoind = Bitcoind::new(&cli, "0.19.1").unwrap();
    let _ = bitcoind.init(5).await;

    let btc = bitcoin::Amount::ONE_BTC;
    let _btc_alice = bitcoin::Amount::ZERO;
    let btc_bob = btc * 10;

    let xmr = 1_000_000_000_000;
    let xmr_alice = xmr * 10;
    let xmr_bob = 0;

    let alice_btc_wallet = Arc::new(
        swap::bitcoin::Wallet::new("alice", &bitcoind.node_url)
            .await
            .unwrap(),
    );
    let bob_btc_wallet = Arc::new(
        swap::bitcoin::Wallet::new("bob", &bitcoind.node_url)
            .await
            .unwrap(),
    );
    bitcoind
        .mint(bob_btc_wallet.0.new_address().await.unwrap(), btc_bob)
        .await
        .unwrap();

    let (monero, _container) = Monero::new(&cli).unwrap();
    monero.init(xmr_alice, xmr_bob).await.unwrap();

    let alice_xmr_wallet = Arc::new(swap::monero::Wallet(monero.alice_wallet_rpc_client()));
    let bob_xmr_wallet = Arc::new(swap::monero::Wallet(monero.bob_wallet_rpc_client()));

    let alice_swap = alice::swap(alice_btc_wallet, alice_xmr_wallet, alice_multiaddr.clone());

    let (cmd_tx, mut _cmd_rx) = mpsc::channel(1);
    let (mut rsp_tx, rsp_rx) = mpsc::channel(1);
    let bob_swap = bob::swap(
        bob_btc_wallet,
        bob_xmr_wallet,
        btc.as_sat(),
        alice_multiaddr,
        cmd_tx,
        rsp_rx,
    );

    rsp_tx.try_send(swap::Rsp::VerifiedAmounts).unwrap();

    try_join(alice_swap, bob_swap).await.unwrap();
}
