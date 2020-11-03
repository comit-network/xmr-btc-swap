#[cfg(not(feature = "tor"))]
mod e2e_test {
    use bitcoin_harness::Bitcoind;
    use futures::{channel::mpsc, future::try_join};
    use libp2p::Multiaddr;
    use monero_harness::Monero;
    use std::sync::Arc;
    use swap::{alice, bob, network::transport::build};
    use testcontainers::clients::Cli;
    use tracing_subscriber::util::SubscriberInitExt;

    #[tokio::test]
    async fn swap() {
        let _guard = tracing_subscriber::fmt()
        .with_env_filter(
            "swap=debug,xmr_btc=debug,hyper=off,reqwest=off,monero_harness=info,testcontainers=info,libp2p=debug",
        )
        .with_ansi(false)
        .set_default();

        let alice_multiaddr: Multiaddr = "/ip4/127.0.0.1/tcp/9876"
            .parse()
            .expect("failed to parse Alice's address");

        let cli = Cli::default();
        let bitcoind = Bitcoind::new(&cli, "0.19.1").unwrap();
        let _ = bitcoind.init(5).await;

        let btc = bitcoin::Amount::from_sat(1_000_000);
        let btc_alice = bitcoin::Amount::ZERO;
        let btc_bob = btc * 10;

        // this xmr value matches the logic of alice::calculate_amounts i.e. btc *
        // 10_000 * 100
        let xmr = 1_000_000_000_000;
        let xmr_alice = xmr * 10;
        let xmr_bob = 0;

        let alice_btc_wallet = Arc::new(
            swap::bitcoin::Wallet::new("alice", bitcoind.node_url.clone())
                .await
                .unwrap(),
        );
        let bob_btc_wallet = Arc::new(
            swap::bitcoin::Wallet::new("bob", bitcoind.node_url.clone())
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

        let alice_behaviour = alice::Alice::default();
        let alice_transport = build(alice_behaviour.identity()).unwrap();
        let alice_swap = alice::swap(
            alice_btc_wallet.clone(),
            alice_xmr_wallet.clone(),
            alice_multiaddr.clone(),
            alice_transport,
            alice_behaviour,
        );

        let (cmd_tx, mut _cmd_rx) = mpsc::channel(1);
        let (mut rsp_tx, rsp_rx) = mpsc::channel(1);
        let bob_behaviour = bob::Bob::default();
        let bob_transport = build(bob_behaviour.identity()).unwrap();
        let bob_swap = bob::swap(
            bob_btc_wallet.clone(),
            bob_xmr_wallet.clone(),
            btc.as_sat(),
            alice_multiaddr,
            cmd_tx,
            rsp_rx,
            bob_transport,
            bob_behaviour,
        );

        // automate the verification step by accepting any amounts sent over by Alice
        rsp_tx.try_send(swap::Rsp::VerifiedAmounts).unwrap();

        try_join(alice_swap, bob_swap).await.unwrap();

        let btc_alice_final = alice_btc_wallet.as_ref().balance().await.unwrap();
        let btc_bob_final = bob_btc_wallet.as_ref().balance().await.unwrap();

        let xmr_alice_final = alice_xmr_wallet.as_ref().get_balance().await.unwrap();

        monero.wait_for_bob_wallet_block_height().await.unwrap();
        let xmr_bob_final = bob_xmr_wallet.as_ref().get_balance().await.unwrap();

        assert_eq!(
            btc_alice_final,
            btc_alice + btc - bitcoin::Amount::from_sat(xmr_btc::bitcoin::TX_FEE)
        );
        assert!(btc_bob_final <= btc_bob - btc);

        assert!(xmr_alice_final.as_piconero() <= xmr_alice - xmr);
        assert_eq!(xmr_bob_final.as_piconero(), xmr_bob + xmr);
    }
}
