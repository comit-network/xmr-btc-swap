pub mod testutils;

use swap::protocol::{alice, bob};
use testcontainers::clients::Cli;
use testutils::{init_tracing, SlowCancelConfig};
use tokio::join;
use futures::future;
use bdk::keys::GeneratableDefaultOptions;
use bdk::database::MemoryDatabase;
use bdk::blockchain::noop_progress;
use url::Url;

/// Run the following tests with RUST_MIN_STACK=10000000

#[tokio::test]
async fn happy_path() {
    testutils::setup_test(SlowCancelConfig, |mut ctx| async move {
        let (alice_swap, _) = ctx.new_swap_as_alice().await;
        let (bob_swap, _) = ctx.new_swap_as_bob().await;

        let alice = alice::run(alice_swap);
        let bob = bob::run(bob_swap);

        let (alice_state, bob_state) = join!(alice, bob);

        ctx.assert_alice_redeemed(alice_state.unwrap()).await;
        ctx.assert_bob_redeemed(bob_state.unwrap()).await;
    })
    .await;
}
//
// const ELECTRUM_RPC_PORT: u16 = 60401;
//
// #[tokio::test]
// async fn happy_path() {
//     let cli = Cli::default();
//
//     let _guard = init_tracing();
//
//     // let config = C::get_config();
//
//     let _c = testutils::init_electrs_container(&cli).await;
//
//
//     let bdk_url = {
//         let input = format!("tcp://@localhost:{}", ELECTRUM_RPC_PORT);
//         Url::parse(&input).unwrap()
//     };
//
//     let client = bdk::electrum_client::Client::new(bdk_url.as_str()).unwrap();
//
//     let blockchain = bdk::blockchain::ElectrumBlockchain::from(client);
//
//     // let bdk_wallet = bdk::Wallet::new(
//     //     "wpkh(tprv8ZgxMBicQKsPdpkqS7Eair4YxjcuuvDPNYmKX3sCniCf16tHEVrjjiSXEkFRnUH77yXc6ZcwHHcLNfjdi5qUvw3VDfgYiH5mNsj5izuiu2N/0/0/*)",
//     //     None,
//     //     Network::Regtest,
//     //     sled::open("/tmp/bdk").expect("could not create sled db").open_tree("default_tree").expect("could not open tree"),
//     //     blockchain).unwrap();
//
//     let p_key = ::bitcoin::PrivateKey::generate_default().expect("could not generate priv key");
//     let bdk_wallet = bdk::Wallet::new(
//         bdk::template::P2WPKH(p_key),
//         None,
//         ::bitcoin::Network::Regtest,
//         MemoryDatabase::default(),
//         blockchain,
//     ).unwrap();
//
//
//     let address = bdk_wallet.get_new_address().unwrap();
//     println!("funded address: {}", address);
//
//     bdk_wallet.sync(noop_progress(), None).unwrap();
//
//     let balance = bdk_wallet.get_balance().unwrap();
//     assert_eq!(0, balance);
//     //future::pending::<()>().await;
// }
