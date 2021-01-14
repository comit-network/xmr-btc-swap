mod alice;
mod bob;

use crate::{
    testutils,
    testutils::{alice::Alice, bob::Bob},
};
use bitcoin_harness::Bitcoind;
use get_port::get_port;
use libp2p::core::Multiaddr;
use monero_harness::{image, Monero};
use std::sync::Arc;
use swap::{bitcoin, config::Config, monero, seed::Seed};

use std::future::Future;
use testcontainers::{clients::Cli, Container};
use tracing_core::dispatcher::DefaultGuard;
use tracing_log::LogTracer;

pub async fn test<T, F>(testfn: T)
where
    T: Fn(Alice, Bob) -> F,
    F: Future<Output = ()>,
{
    let cli = Cli::default();

    let test = Test::new(
        bitcoin::Amount::from_sat(1_000_000),
        monero::Amount::from_piconero(1_000_000_000_000),
        &cli,
    )
    .await;

    testfn(test.alice, test.bob).await
}

pub struct Test<'a> {
    pub alice: Alice,
    pub bob: Bob,
    containers: Containers<'a>,
}

impl<'a> Test<'a> {
    pub async fn new(
        btc_to_swap: bitcoin::Amount,
        xmr_to_swap: monero::Amount,
        cli: &'a Cli,
    ) -> Test<'a> {
        let _guard = init_tracing();

        let (monero, containers) = testutils::init_containers(&cli).await;

        let bob_btc_starting_balance = btc_to_swap * 10;
        let alice_xmr_starting_balance = xmr_to_swap * 10;

        let port = get_port().expect("Failed to find a free port");
        let alice_multiaddr: Multiaddr = format!("/ip4/127.0.0.1/tcp/{}", port)
            .parse()
            .expect("failed to parse Alice's address");

        let config = Config::regtest();
        let alice = Alice::new(
            &containers.bitcoind,
            &monero,
            btc_to_swap,
            xmr_to_swap,
            alice_xmr_starting_balance,
            alice_multiaddr.clone(),
            config,
            Seed::random().unwrap(),
        )
        .await;

        let bob = Bob::new(
            alice_multiaddr,
            alice.peer_id(),
            &containers.bitcoind,
            &monero,
            btc_to_swap,
            xmr_to_swap,
            bob_btc_starting_balance,
            config,
        )
        .await;

        Test {
            alice,
            bob,
            containers,
        }
    }
}

// This is just to keep the containers alive
#[allow(dead_code)]
pub struct Containers<'a> {
    pub bitcoind: Bitcoind<'a>,
    pub monerods: Vec<Container<'a, Cli, image::Monero>>,
}

pub async fn init_containers<'a>(cli: &'a Cli) -> (Monero, Containers<'a>) {
    let bitcoind = Bitcoind::new(cli, "0.19.1").unwrap();
    let _ = bitcoind.init(5).await;
    let (monero, monerods) = Monero::new(&cli, None, vec!["alice".to_string(), "bob".to_string()])
        .await
        .unwrap();

    (monero, Containers { bitcoind, monerods })
}

pub async fn init_wallets(
    name: &str,
    bitcoind: &Bitcoind<'_>,
    monero: &Monero,
    btc_starting_balance: Option<::bitcoin::Amount>,
    xmr_starting_balance: Option<monero::Amount>,
    config: Config,
) -> (Arc<bitcoin::Wallet>, Arc<monero::Wallet>) {
    match xmr_starting_balance {
        Some(amount) => {
            monero
                .init(vec![(name, amount.as_piconero())])
                .await
                .unwrap();
        }
        None => {
            monero
                .init(vec![(name, monero::Amount::ZERO.as_piconero())])
                .await
                .unwrap();
        }
    };

    let xmr_wallet = Arc::new(swap::monero::Wallet {
        inner: monero.wallet(name).unwrap().client(),
        network: config.monero_network,
    });

    let btc_wallet = Arc::new(
        swap::bitcoin::Wallet::new(name, bitcoind.node_url.clone(), config.bitcoin_network)
            .await
            .unwrap(),
    );

    if let Some(amount) = btc_starting_balance {
        bitcoind
            .mint(btc_wallet.inner.new_address().await.unwrap(), amount)
            .await
            .unwrap();
    }

    (btc_wallet, xmr_wallet)
}
/// Utility function to initialize logging in the test environment.
/// Note that you have to keep the `_guard` in scope after calling in test:
///
/// ```rust
/// let _guard = init_tracing();
/// ```
pub fn init_tracing() -> DefaultGuard {
    // converts all log records into tracing events
    // Note: Make sure to initialize without unwrapping, otherwise this causes
    // trouble when running multiple tests.
    let _ = LogTracer::init();

    let global_filter = tracing::Level::WARN;
    let swap_filter = tracing::Level::DEBUG;
    let xmr_btc_filter = tracing::Level::DEBUG;
    let monero_harness_filter = tracing::Level::INFO;
    let bitcoin_harness_filter = tracing::Level::INFO;

    use tracing_subscriber::util::SubscriberInitExt as _;
    tracing_subscriber::fmt()
        .with_env_filter(format!(
            "{},swap={},xmr_btc={},monero_harness={},bitcoin_harness={}",
            global_filter,
            swap_filter,
            xmr_btc_filter,
            monero_harness_filter,
            bitcoin_harness_filter,
        ))
        .set_default()
}
