use monero_sys::{Daemon, WalletHandle};

const STAGENET_REMOTE_NODE: &str = "node.sethforprivacy.com:38089";

#[tokio::test(flavor = "multi_thread")]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("info,test=debug,monero_harness=debug,monero_rpc=debug,wallet_closing=trace,monero_sys=trace,monero_cpp=debug")
        .with_test_writer()
        .init();

    let temp_dir = tempfile::tempdir().unwrap();
    let daemon = Daemon {
        address: STAGENET_REMOTE_NODE.into(),
        ssl: true,
    };

    {
        let wallet = WalletHandle::open_or_create(
            temp_dir.path().join("test_wallet").display().to_string(),
            daemon.clone(),
            monero::Network::Stagenet,
            true,
        )
        .await
        .expect("Failed to create wallet");
        tracing::info!("Dropping wallet");

        std::mem::drop(wallet);
    }

    // Sleep for 2 seconds to allow the wallet to be closed
    tracing::info!("Sleeping for 2 seconds");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    tracing::info!("Closed wallet automatically");

    {
        let wallet = WalletHandle::open_or_create(
            temp_dir.path().join("test_wallet").display().to_string(),
            daemon.clone(),
            monero::Network::Stagenet,
            true,
        )
        .await
        .expect("Failed to create wallet");
        tracing::info!("Dropping wallet");

        std::mem::drop(wallet);
    }

    tracing::info!("Sleeping for 2 seconds");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
}
