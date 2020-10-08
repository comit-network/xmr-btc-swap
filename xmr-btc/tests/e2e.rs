use crate::{
    node::{AliceNode, BobNode},
    transport::Transport,
};
use bitcoin_harness::Bitcoind;
use monero_harness::Monero;
use rand::rngs::OsRng;
use testcontainers::clients::Cli;
use tokio::sync::{
    mpsc,
    mpsc::{Receiver, Sender},
};
use xmr_btc::{alice, bitcoin, bob, monero};

mod node;
mod transport;
mod wallet;

const TEN_XMR: u64 = 10_000_000_000_000;
const RELATIVE_REFUND_TIMELOCK: u32 = 1;
const RELATIVE_PUNISH_TIMELOCK: u32 = 1;

pub async fn init_bitcoind(tc_client: &Cli) -> Bitcoind<'_> {
    let bitcoind = Bitcoind::new(tc_client, "0.19.1").expect("failed to create bitcoind");
    let _ = bitcoind.init(5).await;

    bitcoind
}

pub struct InitialBalances {
    alice_xmr: u64,
    alice_btc: bitcoin::Amount,
    bob_xmr: u64,
    bob_btc: bitcoin::Amount,
}

pub struct SwapAmounts {
    xmr: monero::Amount,
    btc: bitcoin::Amount,
}

pub fn init_alice_and_bob_transports() -> (
    Transport<alice::Message, bob::Message>,
    Transport<bob::Message, alice::Message>,
) {
    let (a_sender, b_receiver): (Sender<alice::Message>, Receiver<alice::Message>) =
        mpsc::channel(5);
    let (b_sender, a_receiver): (Sender<bob::Message>, Receiver<bob::Message>) = mpsc::channel(5);

    let a_transport = Transport {
        sender: a_sender,
        receiver: a_receiver,
    };

    let b_transport = Transport {
        sender: b_sender,
        receiver: b_receiver,
    };

    (a_transport, b_transport)
}

pub async fn init_test<'a>(
    monero: &'a Monero<'a>,
    bitcoind: &Bitcoind<'_>,
) -> (
    alice::State0,
    bob::State0,
    AliceNode<'a>,
    BobNode<'a>,
    InitialBalances,
    SwapAmounts,
) {
    // must be bigger than our hardcoded fee of 10_000
    let btc_amount = bitcoin::Amount::from_sat(10_000_000);
    let xmr_amount = monero::Amount::from_piconero(1_000_000_000_000);

    let swap_amounts = SwapAmounts {
        xmr: xmr_amount,
        btc: btc_amount,
    };

    let fund_alice = TEN_XMR;
    let fund_bob = 0;
    monero.init(fund_alice, fund_bob).await.unwrap();

    let alice_monero_wallet = wallet::monero::AliceWallet(&monero);
    let bob_monero_wallet = wallet::monero::BobWallet(&monero);

    let alice_btc_wallet = wallet::bitcoin::Wallet::new("alice", &bitcoind.node_url)
        .await
        .unwrap();
    let bob_btc_wallet = wallet::bitcoin::make_wallet("bob", &bitcoind, btc_amount)
        .await
        .unwrap();

    let (alice_transport, bob_transport) = init_alice_and_bob_transports();

    let alice = AliceNode::new(alice_transport, alice_btc_wallet, alice_monero_wallet);

    let bob = BobNode::new(bob_transport, bob_btc_wallet, bob_monero_wallet);

    let alice_initial_btc_balance = alice.bitcoin_wallet.balance().await.unwrap();
    let bob_initial_btc_balance = bob.bitcoin_wallet.balance().await.unwrap();

    let alice_initial_xmr_balance = alice.monero_wallet.0.get_balance_alice().await.unwrap();
    let bob_initial_xmr_balance = bob.monero_wallet.0.get_balance_bob().await.unwrap();

    let redeem_address = alice.bitcoin_wallet.new_address().await.unwrap();
    let punish_address = redeem_address.clone();
    let refund_address = bob.bitcoin_wallet.new_address().await.unwrap();

    let alice_state0 = alice::State0::new(
        &mut OsRng,
        btc_amount,
        xmr_amount,
        RELATIVE_REFUND_TIMELOCK,
        RELATIVE_PUNISH_TIMELOCK,
        redeem_address.clone(),
        punish_address.clone(),
    );
    let bob_state0 = bob::State0::new(
        &mut OsRng,
        btc_amount,
        xmr_amount,
        RELATIVE_REFUND_TIMELOCK,
        RELATIVE_PUNISH_TIMELOCK,
        refund_address.clone(),
    );
    let initial_balances = InitialBalances {
        alice_xmr: alice_initial_xmr_balance,
        alice_btc: alice_initial_btc_balance,
        bob_xmr: bob_initial_xmr_balance,
        bob_btc: bob_initial_btc_balance,
    };
    (
        alice_state0,
        bob_state0,
        alice,
        bob,
        initial_balances,
        swap_amounts,
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        init_bitcoind, init_test,
        node::{run_alice_until, run_bob_until},
    };

    use monero_harness::Monero;
    use rand::rngs::OsRng;
    use testcontainers::clients::Cli;

    use futures::future;
    use std::convert::TryInto;
    use tracing_subscriber::util::SubscriberInitExt;
    use xmr_btc::{
        alice, bitcoin,
        bitcoin::{Amount, TX_FEE},
        bob,
    };

    #[tokio::test]
    async fn happy_path() {
        let _guard = tracing_subscriber::fmt()
            .with_env_filter("info")
            .set_default();

        let cli = Cli::default();
        let monero = Monero::new(&cli);
        let bitcoind = init_bitcoind(&cli).await;

        let (
            alice_state0,
            bob_state0,
            mut alice_node,
            mut bob_node,
            initial_balances,
            swap_amounts,
        ) = init_test(&monero, &bitcoind).await;

        let (alice_state, bob_state) = future::try_join(
            run_alice_until(
                &mut alice_node,
                alice_state0.into(),
                alice::is_state6,
                &mut OsRng,
            ),
            run_bob_until(&mut bob_node, bob_state0.into(), bob::is_state5, &mut OsRng),
        )
        .await
        .unwrap();

        let alice_state6: alice::State6 = alice_state.try_into().unwrap();
        let bob_state5: bob::State5 = bob_state.try_into().unwrap();

        let alice_final_btc_balance = alice_node.bitcoin_wallet.balance().await.unwrap();
        let bob_final_btc_balance = bob_node.bitcoin_wallet.balance().await.unwrap();

        let lock_tx_bitcoin_fee = bob_node
            .bitcoin_wallet
            .transaction_fee(bob_state5.tx_lock_id())
            .await
            .unwrap();

        assert_eq!(
            alice_final_btc_balance,
            initial_balances.alice_btc + swap_amounts.btc
                - bitcoin::Amount::from_sat(bitcoin::TX_FEE)
        );
        assert_eq!(
            bob_final_btc_balance,
            initial_balances.bob_btc - swap_amounts.btc - lock_tx_bitcoin_fee
        );

        let alice_final_xmr_balance = alice_node
            .monero_wallet
            .0
            .get_balance_alice()
            .await
            .unwrap();

        bob_node
            .monero_wallet
            .0
            .wait_for_bob_wallet_block_height()
            .await
            .unwrap();

        let bob_final_xmr_balance = bob_node.monero_wallet.0.get_balance_bob().await.unwrap();

        assert_eq!(
            alice_final_xmr_balance,
            initial_balances.alice_xmr
                - u64::from(swap_amounts.xmr)
                - u64::from(alice_state6.lock_xmr_fee())
        );
        assert_eq!(
            bob_final_xmr_balance,
            initial_balances.bob_xmr + u64::from(swap_amounts.xmr)
        );
    }

    #[tokio::test]
    async fn both_refund() {
        let _guard = tracing_subscriber::fmt()
            .with_env_filter("info")
            .set_default();

        let cli = Cli::default();
        let monero = Monero::new(&cli);
        let bitcoind = init_bitcoind(&cli).await;

        let (
            alice_state0,
            bob_state0,
            mut alice_node,
            mut bob_node,
            initial_balances,
            swap_amounts,
        ) = init_test(&monero, &bitcoind).await;

        let (alice_state, bob_state) = future::try_join(
            run_alice_until(
                &mut alice_node,
                alice_state0.into(),
                alice::is_state5,
                &mut OsRng,
            ),
            run_bob_until(&mut bob_node, bob_state0.into(), bob::is_state3, &mut OsRng),
        )
        .await
        .unwrap();

        let alice_state5: alice::State5 = alice_state.try_into().unwrap();
        let bob_state3: bob::State3 = bob_state.try_into().unwrap();

        bob_state3
            .refund_btc(&bob_node.bitcoin_wallet)
            .await
            .unwrap();
        alice_state5
            .refund_xmr(&alice_node.bitcoin_wallet, &alice_node.monero_wallet)
            .await
            .unwrap();

        let alice_final_btc_balance = alice_node.bitcoin_wallet.balance().await.unwrap();
        let bob_final_btc_balance = bob_node.bitcoin_wallet.balance().await.unwrap();

        // lock_tx_bitcoin_fee is determined by the wallet, it is not necessarily equal
        // to TX_FEE
        let lock_tx_bitcoin_fee = bob_node
            .bitcoin_wallet
            .transaction_fee(bob_state3.tx_lock_id())
            .await
            .unwrap();

        assert_eq!(alice_final_btc_balance, initial_balances.alice_btc);
        assert_eq!(
            bob_final_btc_balance,
            // The 2 * TX_FEE corresponds to tx_refund and tx_cancel.
            initial_balances.bob_btc - Amount::from_sat(2 * TX_FEE) - lock_tx_bitcoin_fee
        );

        alice_node
            .monero_wallet
            .0
            .wait_for_alice_wallet_block_height()
            .await
            .unwrap();
        let alice_final_xmr_balance = alice_node
            .monero_wallet
            .0
            .get_balance_alice()
            .await
            .unwrap();
        let bob_final_xmr_balance = bob_node.monero_wallet.0.get_balance_bob().await.unwrap();

        // Because we create a new wallet when claiming Monero, we can only assert on
        // this new wallet owning all of `xmr_amount` after refund
        assert_eq!(alice_final_xmr_balance, u64::from(swap_amounts.xmr));
        assert_eq!(bob_final_xmr_balance, initial_balances.bob_xmr);
    }

    #[tokio::test]
    async fn alice_punishes() {
        let _guard = tracing_subscriber::fmt()
            .with_env_filter("info")
            .set_default();

        let cli = Cli::default();
        let monero = Monero::new(&cli);
        let bitcoind = init_bitcoind(&cli).await;

        let (
            alice_state0,
            bob_state0,
            mut alice_node,
            mut bob_node,
            initial_balances,
            swap_amounts,
        ) = init_test(&monero, &bitcoind).await;

        let (alice_state, bob_state) = future::try_join(
            run_alice_until(
                &mut alice_node,
                alice_state0.into(),
                alice::is_state4,
                &mut OsRng,
            ),
            run_bob_until(&mut bob_node, bob_state0.into(), bob::is_state3, &mut OsRng),
        )
        .await
        .unwrap();

        let alice_state4: alice::State4 = alice_state.try_into().unwrap();
        let bob_state3: bob::State3 = bob_state.try_into().unwrap();

        alice_state4
            .punish(&alice_node.bitcoin_wallet)
            .await
            .unwrap();

        let alice_final_btc_balance = alice_node.bitcoin_wallet.balance().await.unwrap();
        let bob_final_btc_balance = bob_node.bitcoin_wallet.balance().await.unwrap();

        // lock_tx_bitcoin_fee is determined by the wallet, it is not necessarily equal
        // to TX_FEE
        let lock_tx_bitcoin_fee = bob_node
            .bitcoin_wallet
            .transaction_fee(bob_state3.tx_lock_id())
            .await
            .unwrap();

        assert_eq!(
            alice_final_btc_balance,
            initial_balances.alice_btc + swap_amounts.btc - Amount::from_sat(2 * TX_FEE)
        );
        assert_eq!(
            bob_final_btc_balance,
            initial_balances.bob_btc - swap_amounts.btc - lock_tx_bitcoin_fee
        );
    }
}
