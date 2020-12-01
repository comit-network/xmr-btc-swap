pub mod node;
pub mod transport;
pub mod wallet;

pub mod bob {
    use xmr_btc::bob::State;

    pub fn is_state2(state: &State) -> bool {
        matches!(state, State::State2 { .. })
    }

    // TODO: use macro or generics
    pub fn is_state5(state: &State) -> bool {
        matches!(state, State::State5 { .. })
    }

    // TODO: use macro or generics
    pub fn is_state3(state: &State) -> bool {
        matches!(state, State::State3 { .. })
    }
}

pub mod alice {
    use xmr_btc::alice::State;

    pub fn is_state3(state: &State) -> bool {
        matches!(state, State::State3 { .. })
    }

    // TODO: use macro or generics
    pub fn is_state4(state: &State) -> bool {
        matches!(state, State::State4 { .. })
    }

    // TODO: use macro or generics
    pub fn is_state5(state: &State) -> bool {
        matches!(state, State::State5 { .. })
    }

    // TODO: use macro or generics
    pub fn is_state6(state: &State) -> bool {
        matches!(state, State::State6 { .. })
    }
}

use bitcoin_harness::Bitcoind;
use monero_harness::Monero;
use node::{AliceNode, BobNode};
use rand::rngs::OsRng;
use testcontainers::clients::Cli;
use tokio::sync::{
    mpsc,
    mpsc::{Receiver, Sender},
};
use transport::Transport;
use xmr_btc::{bitcoin, monero};

const TEN_XMR: u64 = 10_000_000_000_000;
const RELATIVE_REFUND_TIMELOCK: u32 = 1;
const RELATIVE_PUNISH_TIMELOCK: u32 = 1;
pub const ALICE_TEST_DB_FOLDER: &str = "../target/e2e-test-alice-recover";
pub const BOB_TEST_DB_FOLDER: &str = "../target/e2e-test-bob-recover";

pub async fn init_bitcoind(tc_client: &Cli) -> Bitcoind<'_> {
    let bitcoind = Bitcoind::new(tc_client, "0.19.1").expect("failed to create bitcoind");
    let _ = bitcoind.init(5).await;

    bitcoind
}

pub struct InitialBalances {
    pub alice_xmr: monero::Amount,
    pub alice_btc: bitcoin::Amount,
    pub bob_xmr: monero::Amount,
    pub bob_btc: bitcoin::Amount,
}

pub struct SwapAmounts {
    pub xmr: monero::Amount,
    pub btc: bitcoin::Amount,
}

pub fn init_alice_and_bob_transports() -> (
    Transport<xmr_btc::alice::Message, xmr_btc::bob::Message>,
    Transport<xmr_btc::bob::Message, xmr_btc::alice::Message>,
) {
    let (a_sender, b_receiver): (
        Sender<xmr_btc::alice::Message>,
        Receiver<xmr_btc::alice::Message>,
    ) = mpsc::channel(5);
    let (b_sender, a_receiver): (
        Sender<xmr_btc::bob::Message>,
        Receiver<xmr_btc::bob::Message>,
    ) = mpsc::channel(5);

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

pub async fn init_test(
    monero: &Monero,
    bitcoind: &Bitcoind<'_>,
    refund_timelock: Option<u32>,
    punish_timelock: Option<u32>,
) -> (
    xmr_btc::alice::State0,
    xmr_btc::bob::State0,
    AliceNode,
    BobNode,
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
    monero
        .init(vec![("alice", fund_alice), ("bob", fund_bob)])
        .await
        .unwrap();

    let alice_monero_wallet = wallet::monero::Wallet(monero.wallet("alice").unwrap().client());
    let bob_monero_wallet = wallet::monero::Wallet(monero.wallet("bob").unwrap().client());

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

    let alice_initial_xmr_balance = alice.monero_wallet.get_balance().await.unwrap();
    let bob_initial_xmr_balance = bob.monero_wallet.get_balance().await.unwrap();

    let redeem_address = alice.bitcoin_wallet.new_address().await.unwrap();
    let punish_address = redeem_address.clone();
    let refund_address = bob.bitcoin_wallet.new_address().await.unwrap();

    let alice_state0 = xmr_btc::alice::State0::new(
        &mut OsRng,
        btc_amount,
        xmr_amount,
        refund_timelock.unwrap_or(RELATIVE_REFUND_TIMELOCK),
        punish_timelock.unwrap_or(RELATIVE_PUNISH_TIMELOCK),
        redeem_address.clone(),
        punish_address.clone(),
    );
    let bob_state0 = xmr_btc::bob::State0::new(
        &mut OsRng,
        btc_amount,
        xmr_amount,
        refund_timelock.unwrap_or(RELATIVE_REFUND_TIMELOCK),
        punish_timelock.unwrap_or(RELATIVE_PUNISH_TIMELOCK),
        refund_address,
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
