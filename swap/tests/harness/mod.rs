mod bitcoind;
mod electrs;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use bitcoin_harness::{BitcoindRpcApi, Client};
use futures::Future;
use get_port::get_port;
use libp2p::core::Multiaddr;
use libp2p::PeerId;
use monero_harness::{image, Monero};
use std::cmp::Ordering;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use swap::bitcoin::{CancelTimelock, PunishTimelock, TxCancel, TxPunish, TxRedeem, TxRefund};
use swap::database::Database;
use swap::env::{Config, GetConfig};
use swap::network::swarm;
use swap::protocol::alice::event_loop::FixedRate;
use swap::protocol::alice::{AliceState, Swap};
use swap::protocol::bob::BobState;
use swap::protocol::{alice, bob};
use swap::seed::Seed;
use swap::{bitcoin, env, monero};
use tempfile::tempdir;
use testcontainers::clients::Cli;
use testcontainers::{Container, Docker, RunArgs};
use tokio::sync::mpsc;
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;
use tokio::time::{interval, timeout};
use tracing_subscriber::util::SubscriberInitExt;
use url::Url;
use uuid::Uuid;

pub async fn setup_test<T, F, C>(_config: C, testfn: T)
where
    T: Fn(TestContext) -> F,
    F: Future<Output = Result<()>>,
    C: GetConfig,
{
    let cli = Cli::default();

    let _guard = tracing_subscriber::fmt()
        .with_env_filter("warn,swap=debug,monero_harness=debug,monero_rpc=debug,bitcoin_harness=info,testcontainers=info") // add `reqwest::connect::verbose=trace` if you want to logs of the RPC clients
        .with_test_writer()
        .set_default();

    let env_config = C::get_config();

    let (monero, containers) = init_containers(&cli).await;
    monero.init_miner().await.unwrap();

    let btc_amount = bitcoin::Amount::from_sat(1_000_000);
    let xmr_amount = monero::Amount::from_monero(btc_amount.as_btc() / FixedRate::RATE).unwrap();

    let alice_starting_balances =
        StartingBalances::new(bitcoin::Amount::ZERO, xmr_amount, Some(10));

    let electrs_rpc_port = containers
        .electrs
        .get_host_port(electrs::RPC_PORT)
        .expect("Could not map electrs rpc port");

    let alice_seed = Seed::random().unwrap();
    let (alice_bitcoin_wallet, alice_monero_wallet) = init_test_wallets(
        MONERO_WALLET_NAME_ALICE,
        containers.bitcoind_url.clone(),
        &monero,
        alice_starting_balances.clone(),
        tempdir().unwrap().path(),
        electrs_rpc_port,
        &alice_seed,
        env_config,
    )
    .await;

    let alice_listen_port = get_port().expect("Failed to find a free port");
    let alice_listen_address: Multiaddr = format!("/ip4/127.0.0.1/tcp/{}", alice_listen_port)
        .parse()
        .expect("failed to parse Alice's address");

    let alice_db_path = tempdir().unwrap().into_path();
    let (alice_handle, alice_swap_handle) = start_alice(
        &alice_seed,
        alice_db_path.clone(),
        alice_listen_address.clone(),
        env_config,
        alice_bitcoin_wallet.clone(),
        alice_monero_wallet.clone(),
    )
    .await;

    let bob_seed = Seed::random().unwrap();
    let bob_starting_balances = StartingBalances::new(btc_amount * 10, monero::Amount::ZERO, None);

    let (bob_bitcoin_wallet, bob_monero_wallet) = init_test_wallets(
        MONERO_WALLET_NAME_BOB,
        containers.bitcoind_url,
        &monero,
        bob_starting_balances.clone(),
        tempdir().unwrap().path(),
        electrs_rpc_port,
        &bob_seed,
        env_config,
    )
    .await;

    let bob_params = BobParams {
        seed: Seed::random().unwrap(),
        db_path: tempdir().unwrap().path().to_path_buf(),
        bitcoin_wallet: bob_bitcoin_wallet.clone(),
        monero_wallet: bob_monero_wallet.clone(),
        alice_address: alice_listen_address.clone(),
        alice_peer_id: alice_handle.peer_id,
        env_config,
    };

    monero.start_miner().await.unwrap();

    let test = TestContext {
        env_config,
        btc_amount,
        xmr_amount,
        alice_seed,
        alice_db_path,
        alice_listen_address,
        alice_starting_balances,
        alice_bitcoin_wallet,
        alice_monero_wallet,
        alice_swap_handle,
        alice_handle,
        bob_params,
        bob_starting_balances,
        bob_bitcoin_wallet,
        bob_monero_wallet,
    };

    testfn(test).await.unwrap()
}

async fn init_containers(cli: &Cli) -> (Monero, Containers<'_>) {
    let prefix = random_prefix();
    let bitcoind_name = format!("{}_{}", prefix, "bitcoind");
    let (bitcoind, bitcoind_url) =
        init_bitcoind_container(&cli, prefix.clone(), bitcoind_name.clone(), prefix.clone())
            .await
            .expect("could not init bitcoind");
    let electrs = init_electrs_container(&cli, prefix.clone(), bitcoind_name, prefix)
        .await
        .expect("could not init electrs");
    let (monero, monerod_container, monero_wallet_rpc_containers) =
        Monero::new(&cli, vec![MONERO_WALLET_NAME_ALICE, MONERO_WALLET_NAME_BOB])
            .await
            .unwrap();

    (monero, Containers {
        bitcoind_url,
        bitcoind,
        monerod_container,
        monero_wallet_rpc_containers,
        electrs,
    })
}

async fn init_bitcoind_container(
    cli: &Cli,
    volume: String,
    name: String,
    network: String,
) -> Result<(Container<'_, Cli, bitcoind::Bitcoind>, Url)> {
    let image = bitcoind::Bitcoind::default().with_volume(volume);

    let run_args = RunArgs::default().with_name(name).with_network(network);

    let docker = cli.run_with_args(image, run_args);
    let a = docker
        .get_host_port(bitcoind::RPC_PORT)
        .context("Could not map bitcoind rpc port")?;

    let bitcoind_url = {
        let input = format!(
            "http://{}:{}@localhost:{}",
            bitcoind::RPC_USER,
            bitcoind::RPC_PASSWORD,
            a
        );
        Url::parse(&input).unwrap()
    };

    init_bitcoind(bitcoind_url.clone(), 5).await?;

    Ok((docker, bitcoind_url.clone()))
}

pub async fn init_electrs_container(
    cli: &Cli,
    volume: String,
    bitcoind_container_name: String,
    network: String,
) -> Result<Container<'_, Cli, electrs::Electrs>> {
    let bitcoind_rpc_addr = format!("{}:{}", bitcoind_container_name, bitcoind::RPC_PORT);
    let image = electrs::Electrs::default()
        .with_volume(volume)
        .with_daemon_rpc_addr(bitcoind_rpc_addr)
        .with_tag("latest");

    let run_args = RunArgs::default().with_network(network);

    let docker = cli.run_with_args(image, run_args);

    Ok(docker)
}

async fn start_alice(
    seed: &Seed,
    db_path: PathBuf,
    listen_address: Multiaddr,
    env_config: Config,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,
) -> (AliceApplicationHandle, Receiver<alice::Swap>) {
    let db = Arc::new(Database::open(db_path.as_path()).unwrap());

    let current_balance = monero_wallet.get_balance().await.unwrap();
    let lock_fee = monero_wallet.static_tx_fee_estimate();
    let max_buy = bitcoin::Amount::from_sat(u64::MAX);
    let latest_rate = FixedRate::default();
    let resume_only = false;

    let mut swarm = swarm::alice(
        &seed,
        current_balance,
        lock_fee,
        max_buy,
        latest_rate,
        resume_only,
    )
    .unwrap();
    swarm.listen_on(listen_address).unwrap();

    let (event_loop, swap_handle) = alice::EventLoop::new(
        swarm,
        env_config,
        bitcoin_wallet,
        monero_wallet,
        db,
        FixedRate::default(),
        bitcoin::Amount::ONE_BTC,
    )
    .unwrap();

    let peer_id = event_loop.peer_id();
    let handle = tokio::spawn(event_loop.run());

    (AliceApplicationHandle { handle, peer_id }, swap_handle)
}

#[allow(clippy::too_many_arguments)]
async fn init_test_wallets(
    name: &str,
    bitcoind_url: Url,
    monero: &Monero,
    starting_balances: StartingBalances,
    datadir: &Path,
    electrum_rpc_port: u16,
    seed: &Seed,
    env_config: Config,
) -> (Arc<bitcoin::Wallet>, Arc<monero::Wallet>) {
    monero
        .init_wallet(
            name,
            starting_balances
                .xmr_outputs
                .into_iter()
                .map(|amount| amount.as_piconero())
                .collect(),
        )
        .await
        .unwrap();

    let xmr_wallet = swap::monero::Wallet::connect(
        monero.wallet(name).unwrap().client().clone(),
        name.to_string(),
        env_config,
    )
    .await
    .unwrap();

    let electrum_rpc_url = {
        let input = format!("tcp://@localhost:{}", electrum_rpc_port);
        Url::parse(&input).unwrap()
    };

    let btc_wallet = swap::bitcoin::Wallet::new(
        electrum_rpc_url,
        datadir,
        seed.derive_extended_private_key(env_config.bitcoin_network)
            .expect("Could not create extended private key from seed"),
        env_config,
        1,
    )
    .await
    .expect("could not init btc wallet");

    if starting_balances.btc != bitcoin::Amount::ZERO {
        mint(
            bitcoind_url,
            btc_wallet.new_address().await.unwrap(),
            starting_balances.btc,
        )
        .await
        .expect("could not mint btc starting balance");

        let mut interval = interval(Duration::from_secs(1u64));
        let mut retries = 0u8;
        let max_retries = 30u8;
        loop {
            retries += 1;
            btc_wallet.sync().await.unwrap();

            let btc_balance = btc_wallet.balance().await.unwrap();

            if btc_balance == starting_balances.btc {
                break;
            } else if retries == max_retries {
                panic!(
                    "Bitcoin wallet initialization failed, reached max retries upon balance sync"
                )
            }

            interval.tick().await;
        }
    }

    (Arc::new(btc_wallet), Arc::new(xmr_wallet))
}

const MONERO_WALLET_NAME_BOB: &str = "bob";
const MONERO_WALLET_NAME_ALICE: &str = "alice";
const BITCOIN_TEST_WALLET_NAME: &str = "testwallet";

#[derive(Debug, Clone)]
pub struct StartingBalances {
    pub xmr: monero::Amount,
    pub xmr_outputs: Vec<monero::Amount>,
    pub btc: bitcoin::Amount,
}

impl StartingBalances {
    /// If monero_outputs is specified the monero balance will be:
    /// monero_outputs * new_xmr = self_xmr
    pub fn new(btc: bitcoin::Amount, xmr: monero::Amount, monero_outputs: Option<u64>) -> Self {
        match monero_outputs {
            None => {
                if xmr == monero::Amount::ZERO {
                    return Self {
                        xmr,
                        xmr_outputs: vec![],
                        btc,
                    };
                }

                Self {
                    xmr,
                    xmr_outputs: vec![xmr],
                    btc,
                }
            }
            Some(outputs) => {
                let mut xmr_outputs = Vec::new();
                let mut sum_xmr = monero::Amount::ZERO;

                for _ in 0..outputs {
                    xmr_outputs.push(xmr);
                    sum_xmr = sum_xmr + xmr;
                }

                Self {
                    xmr: sum_xmr,
                    xmr_outputs,
                    btc,
                }
            }
        }
    }
}

struct BobParams {
    seed: Seed,
    db_path: PathBuf,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    monero_wallet: Arc<monero::Wallet>,
    alice_address: Multiaddr,
    alice_peer_id: PeerId,
    env_config: Config,
}

impl BobParams {
    pub async fn new_swap_from_db(&self, swap_id: Uuid) -> Result<(bob::Swap, bob::EventLoop)> {
        let (event_loop, handle) = self.new_eventloop(swap_id).await?;
        let db = Database::open(&self.db_path)?;

        let swap = bob::Swap::from_db(
            db,
            swap_id,
            self.bitcoin_wallet.clone(),
            self.monero_wallet.clone(),
            self.env_config,
            handle,
            self.monero_wallet.get_main_address(),
        )?;

        Ok((swap, event_loop))
    }

    pub async fn new_swap(
        &self,
        btc_amount: bitcoin::Amount,
    ) -> Result<(bob::Swap, bob::EventLoop)> {
        let swap_id = Uuid::new_v4();

        let (event_loop, handle) = self.new_eventloop(swap_id).await?;
        let db = Database::open(&self.db_path)?;

        let swap = bob::Swap::new(
            db,
            swap_id,
            self.bitcoin_wallet.clone(),
            self.monero_wallet.clone(),
            self.env_config,
            handle,
            self.monero_wallet.get_main_address(),
            btc_amount,
        );

        Ok((swap, event_loop))
    }

    pub async fn new_eventloop(
        &self,
        swap_id: Uuid,
    ) -> Result<(bob::EventLoop, bob::EventLoopHandle)> {
        let tor_socks5_port = get_port()
            .expect("We don't care about Tor in the tests so we get a free port to disable it.");
        let mut swarm = swarm::bob(&self.seed, self.alice_peer_id, tor_socks5_port).await?;
        swarm
            .behaviour_mut()
            .add_address(self.alice_peer_id, self.alice_address.clone());

        bob::EventLoop::new(
            swap_id,
            swarm,
            self.alice_peer_id,
            self.bitcoin_wallet.clone(),
        )
    }
}

pub struct BobApplicationHandle(JoinHandle<()>);

impl BobApplicationHandle {
    pub fn abort(&self) {
        self.0.abort()
    }
}

pub struct AliceApplicationHandle {
    handle: JoinHandle<()>,
    peer_id: PeerId,
}

impl AliceApplicationHandle {
    pub fn abort(&self) {
        self.handle.abort()
    }
}

pub struct TestContext {
    env_config: Config,

    btc_amount: bitcoin::Amount,
    xmr_amount: monero::Amount,

    alice_seed: Seed,
    alice_db_path: PathBuf,
    alice_listen_address: Multiaddr,

    alice_starting_balances: StartingBalances,
    alice_bitcoin_wallet: Arc<bitcoin::Wallet>,
    alice_monero_wallet: Arc<monero::Wallet>,
    alice_swap_handle: mpsc::Receiver<Swap>,
    alice_handle: AliceApplicationHandle,

    bob_params: BobParams,
    bob_starting_balances: StartingBalances,
    bob_bitcoin_wallet: Arc<bitcoin::Wallet>,
    bob_monero_wallet: Arc<monero::Wallet>,
}

impl TestContext {
    pub async fn restart_alice(&mut self) {
        self.alice_handle.abort();

        let (alice_handle, alice_swap_handle) = start_alice(
            &self.alice_seed,
            self.alice_db_path.clone(),
            self.alice_listen_address.clone(),
            self.env_config,
            self.alice_bitcoin_wallet.clone(),
            self.alice_monero_wallet.clone(),
        )
        .await;

        self.alice_handle = alice_handle;
        self.alice_swap_handle = alice_swap_handle;
    }

    pub async fn alice_next_swap(&mut self) -> alice::Swap {
        timeout(Duration::from_secs(20), self.alice_swap_handle.recv())
            .await
            .expect("No Alice swap within 20 seconds, aborting because this test is likely waiting for a swap forever...")
            .unwrap()
    }

    pub async fn bob_swap(&mut self) -> (bob::Swap, BobApplicationHandle) {
        let (swap, event_loop) = self.bob_params.new_swap(self.btc_amount).await.unwrap();

        // ensure the wallet is up to date for concurrent swap tests
        swap.bitcoin_wallet.sync().await.unwrap();

        let join_handle = tokio::spawn(event_loop.run());

        (swap, BobApplicationHandle(join_handle))
    }

    pub async fn stop_and_resume_bob_from_db(
        &mut self,
        join_handle: BobApplicationHandle,
        swap_id: Uuid,
    ) -> (bob::Swap, BobApplicationHandle) {
        join_handle.abort();

        let (swap, event_loop) = self.bob_params.new_swap_from_db(swap_id).await.unwrap();

        let join_handle = tokio::spawn(event_loop.run());

        (swap, BobApplicationHandle(join_handle))
    }

    pub async fn assert_alice_redeemed(&mut self, state: AliceState) {
        assert!(matches!(state, AliceState::BtcRedeemed));

        assert_eventual_balance(
            self.alice_bitcoin_wallet.as_ref(),
            Ordering::Equal,
            self.alice_redeemed_btc_balance().await,
        )
        .await
        .unwrap();

        assert_eventual_balance(
            self.alice_monero_wallet.as_ref(),
            Ordering::Less,
            self.alice_redeemed_xmr_balance(),
        )
        .await
        .unwrap();
    }

    pub async fn assert_alice_refunded(&mut self, state: AliceState) {
        assert!(matches!(state, AliceState::XmrRefunded));

        assert_eventual_balance(
            self.alice_bitcoin_wallet.as_ref(),
            Ordering::Equal,
            self.alice_refunded_btc_balance(),
        )
        .await
        .unwrap();

        // Alice pays fees - comparison does not take exact lock fee into account
        assert_eventual_balance(
            self.alice_monero_wallet.as_ref(),
            Ordering::Greater,
            self.alice_refunded_xmr_balance(),
        )
        .await
        .unwrap();
    }

    pub async fn assert_alice_punished(&self, state: AliceState) {
        assert!(matches!(state, AliceState::BtcPunished));

        assert_eventual_balance(
            self.alice_bitcoin_wallet.as_ref(),
            Ordering::Equal,
            self.alice_punished_btc_balance().await,
        )
        .await
        .unwrap();

        assert_eventual_balance(
            self.alice_monero_wallet.as_ref(),
            Ordering::Less,
            self.alice_punished_xmr_balance(),
        )
        .await
        .unwrap();
    }

    pub async fn assert_bob_redeemed(&self, state: BobState) {
        assert_eventual_balance(
            self.bob_bitcoin_wallet.as_ref(),
            Ordering::Equal,
            self.bob_redeemed_btc_balance(state).await.unwrap(),
        )
        .await
        .unwrap();

        // unload the generated wallet by opening the original wallet
        self.bob_monero_wallet.re_open().await.unwrap();

        assert_eventual_balance(
            self.bob_monero_wallet.as_ref(),
            Ordering::Greater,
            self.bob_redeemed_xmr_balance(),
        )
        .await
        .unwrap();
    }

    pub async fn assert_bob_refunded(&self, state: BobState) {
        self.bob_bitcoin_wallet.sync().await.unwrap();

        let lock_tx_id = if let BobState::BtcRefunded(state4) = state {
            state4.tx_lock_id()
        } else {
            panic!("Bob in not in btc refunded state: {:?}", state);
        };
        let lock_tx_bitcoin_fee = self
            .bob_bitcoin_wallet
            .transaction_fee(lock_tx_id)
            .await
            .unwrap();

        let btc_balance_after_swap = self.bob_bitcoin_wallet.balance().await.unwrap();

        let cancel_fee = self
            .alice_bitcoin_wallet
            .estimate_fee(TxCancel::weight(), self.btc_amount)
            .await
            .expect("To estimate fee correctly");
        let refund_fee = self
            .alice_bitcoin_wallet
            .estimate_fee(TxRefund::weight(), self.btc_amount)
            .await
            .expect("To estimate fee correctly");

        let bob_cancelled_and_refunded = btc_balance_after_swap
            == self.bob_starting_balances.btc - lock_tx_bitcoin_fee - cancel_fee - refund_fee;

        assert!(bob_cancelled_and_refunded);

        assert_eventual_balance(
            self.bob_monero_wallet.as_ref(),
            Ordering::Equal,
            self.bob_refunded_xmr_balance(),
        )
        .await
        .unwrap();
    }

    pub async fn assert_bob_punished(&self, state: BobState) {
        assert_eventual_balance(
            self.bob_bitcoin_wallet.as_ref(),
            Ordering::Equal,
            self.bob_punished_btc_balance(state).await.unwrap(),
        )
        .await
        .unwrap();

        assert_eventual_balance(
            self.bob_monero_wallet.as_ref(),
            Ordering::Equal,
            self.bob_punished_xmr_balance(),
        )
        .await
        .unwrap();
    }

    fn alice_redeemed_xmr_balance(&self) -> monero::Amount {
        self.alice_starting_balances.xmr - self.xmr_amount
    }

    async fn alice_redeemed_btc_balance(&self) -> bitcoin::Amount {
        let fee = self
            .alice_bitcoin_wallet
            .estimate_fee(TxRedeem::weight(), self.btc_amount)
            .await
            .expect("To estimate fee correctly");
        self.alice_starting_balances.btc + self.btc_amount - fee
    }

    fn bob_redeemed_xmr_balance(&self) -> monero::Amount {
        self.bob_starting_balances.xmr
    }

    async fn bob_redeemed_btc_balance(&self, state: BobState) -> Result<bitcoin::Amount> {
        self.bob_bitcoin_wallet.sync().await?;

        let lock_tx_id = if let BobState::XmrRedeemed { tx_lock_id } = state {
            tx_lock_id
        } else {
            bail!("Bob in not in xmr redeemed state: {:?}", state);
        };

        let lock_tx_bitcoin_fee = self.bob_bitcoin_wallet.transaction_fee(lock_tx_id).await?;

        Ok(self.bob_starting_balances.btc - self.btc_amount - lock_tx_bitcoin_fee)
    }

    fn alice_refunded_xmr_balance(&self) -> monero::Amount {
        self.alice_starting_balances.xmr - self.xmr_amount
    }

    fn alice_refunded_btc_balance(&self) -> bitcoin::Amount {
        self.alice_starting_balances.btc
    }

    fn bob_refunded_xmr_balance(&self) -> monero::Amount {
        self.bob_starting_balances.xmr
    }

    fn alice_punished_xmr_balance(&self) -> monero::Amount {
        self.alice_starting_balances.xmr - self.xmr_amount
    }

    async fn alice_punished_btc_balance(&self) -> bitcoin::Amount {
        let cancel_fee = self
            .alice_bitcoin_wallet
            .estimate_fee(TxCancel::weight(), self.btc_amount)
            .await
            .expect("To estimate fee correctly");
        let punish_fee = self
            .alice_bitcoin_wallet
            .estimate_fee(TxPunish::weight(), self.btc_amount)
            .await
            .expect("To estimate fee correctly");
        self.alice_starting_balances.btc + self.btc_amount - cancel_fee - punish_fee
    }

    fn bob_punished_xmr_balance(&self) -> monero::Amount {
        self.bob_starting_balances.xmr
    }

    async fn bob_punished_btc_balance(&self, state: BobState) -> Result<bitcoin::Amount> {
        self.bob_bitcoin_wallet.sync().await?;

        let lock_tx_id = if let BobState::BtcPunished { tx_lock_id } = state {
            tx_lock_id
        } else {
            bail!("Bob in not in btc punished state: {:?}", state);
        };

        let lock_tx_bitcoin_fee = self.bob_bitcoin_wallet.transaction_fee(lock_tx_id).await?;

        Ok(self.bob_starting_balances.btc - self.btc_amount - lock_tx_bitcoin_fee)
    }
}

async fn assert_eventual_balance<A: fmt::Display + PartialOrd>(
    wallet: &impl Wallet<Amount = A>,
    ordering: Ordering,
    expected: A,
) -> Result<()> {
    let ordering_str = match ordering {
        Ordering::Less => "less than",
        Ordering::Equal => "equal to",
        Ordering::Greater => "greater than",
    };

    let mut current_balance = wallet.get_balance().await?;

    let assertion = async {
        while current_balance.partial_cmp(&expected).unwrap() != ordering {
            tokio::time::sleep(Duration::from_millis(500)).await;

            wallet.refresh().await?;
            current_balance = wallet.get_balance().await?;
        }

        tracing::debug!(
            "Assertion successful! Balance {} is {} {}",
            current_balance,
            ordering_str,
            expected
        );

        Result::<_, anyhow::Error>::Ok(())
    };

    let timeout = Duration::from_secs(10);

    tokio::time::timeout(timeout, assertion)
        .await
        .with_context(|| {
            format!(
                "Expected balance to be {} {} after at most {}s but was {}",
                ordering_str,
                expected,
                timeout.as_secs(),
                current_balance
            )
        })??;

    Ok(())
}

#[async_trait]
trait Wallet {
    type Amount;

    async fn refresh(&self) -> Result<()>;
    async fn get_balance(&self) -> Result<Self::Amount>;
}

#[async_trait]
impl Wallet for monero::Wallet {
    type Amount = monero::Amount;

    async fn refresh(&self) -> Result<()> {
        self.refresh().await?;

        Ok(())
    }

    async fn get_balance(&self) -> Result<Self::Amount> {
        self.get_balance().await
    }
}

#[async_trait]
impl Wallet for bitcoin::Wallet {
    type Amount = bitcoin::Amount;

    async fn refresh(&self) -> Result<()> {
        self.sync().await
    }

    async fn get_balance(&self) -> Result<Self::Amount> {
        self.balance().await
    }
}

fn random_prefix() -> String {
    use rand::distributions::Alphanumeric;
    use rand::{thread_rng, Rng};
    use std::iter;
    const LEN: usize = 8;
    let mut rng = thread_rng();
    let chars: String = iter::repeat(())
        .map(|()| rng.sample(Alphanumeric))
        .map(char::from)
        .take(LEN)
        .collect();
    chars
}

async fn mine(bitcoind_client: Client, reward_address: bitcoin::Address) -> Result<()> {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        bitcoind_client
            .generatetoaddress(1, reward_address.clone(), None)
            .await?;
    }
}

async fn init_bitcoind(node_url: Url, spendable_quantity: u32) -> Result<Client> {
    let bitcoind_client = Client::new(node_url.clone());

    bitcoind_client
        .createwallet(BITCOIN_TEST_WALLET_NAME, None, None, None, None)
        .await?;

    let reward_address = bitcoind_client
        .with_wallet(BITCOIN_TEST_WALLET_NAME)?
        .getnewaddress(None, None)
        .await?;

    bitcoind_client
        .generatetoaddress(101 + spendable_quantity, reward_address.clone(), None)
        .await?;
    let _ = tokio::spawn(mine(bitcoind_client.clone(), reward_address));
    Ok(bitcoind_client)
}

/// Send Bitcoin to the specified address, limited to the spendable bitcoin
/// quantity.
pub async fn mint(node_url: Url, address: bitcoin::Address, amount: bitcoin::Amount) -> Result<()> {
    let bitcoind_client = Client::new(node_url.clone());

    bitcoind_client
        .send_to_address(BITCOIN_TEST_WALLET_NAME, address.clone(), amount)
        .await?;

    // Confirm the transaction
    let reward_address = bitcoind_client
        .with_wallet(BITCOIN_TEST_WALLET_NAME)?
        .getnewaddress(None, None)
        .await?;
    bitcoind_client
        .generatetoaddress(1, reward_address, None)
        .await?;

    Ok(())
}

// This is just to keep the containers alive
#[allow(dead_code)]
struct Containers<'a> {
    bitcoind_url: Url,
    bitcoind: Container<'a, Cli, bitcoind::Bitcoind>,
    monerod_container: Container<'a, Cli, image::Monerod>,
    monero_wallet_rpc_containers: Vec<Container<'a, Cli, image::MoneroWalletRpc>>,
    electrs: Container<'a, Cli, electrs::Electrs>,
}

pub mod alice_run_until {
    use swap::protocol::alice::AliceState;

    pub fn is_xmr_lock_transaction_sent(state: &AliceState) -> bool {
        matches!(state, AliceState::XmrLockTransactionSent { .. })
    }

    pub fn is_encsig_learned(state: &AliceState) -> bool {
        matches!(state, AliceState::EncSigLearned { .. })
    }
}

pub mod bob_run_until {
    use swap::protocol::bob::BobState;

    pub fn is_btc_locked(state: &BobState) -> bool {
        matches!(state, BobState::BtcLocked(..))
    }

    pub fn is_lock_proof_received(state: &BobState) -> bool {
        matches!(state, BobState::XmrLockProofReceived { .. })
    }

    pub fn is_xmr_locked(state: &BobState) -> bool {
        matches!(state, BobState::XmrLocked(..))
    }

    pub fn is_encsig_sent(state: &BobState) -> bool {
        matches!(state, BobState::EncSigSent(..))
    }
}

pub struct SlowCancelConfig;

impl GetConfig for SlowCancelConfig {
    fn get_config() -> Config {
        Config {
            bitcoin_cancel_timelock: CancelTimelock::new(180),
            ..env::Regtest::get_config()
        }
    }
}

pub struct FastCancelConfig;

impl GetConfig for FastCancelConfig {
    fn get_config() -> Config {
        Config {
            bitcoin_cancel_timelock: CancelTimelock::new(10),
            ..env::Regtest::get_config()
        }
    }
}

pub struct FastPunishConfig;

impl GetConfig for FastPunishConfig {
    fn get_config() -> Config {
        Config {
            bitcoin_cancel_timelock: CancelTimelock::new(10),
            bitcoin_punish_timelock: PunishTimelock::new(10),
            ..env::Regtest::get_config()
        }
    }
}
