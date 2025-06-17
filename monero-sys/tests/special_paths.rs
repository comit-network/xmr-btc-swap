use monero::Network;
use monero_sys::{Daemon, WalletHandle};
use tempfile::tempdir;

#[tokio::test]
async fn test_wallet_with_special_paths() {
    let tempdir = tempdir().unwrap();

    let special_paths = vec![
        "path_with_unicode_漢字",
        "path_with_emoji_😊",
        "path with space",
        "path-with-hyphen",
    ];

    let daemon = Daemon {
        address: "https://moneronode.org:18081".into(),
        ssl: true,
    };

    let futures = special_paths
        .into_iter()
        .map(|path| {
            let path = tempdir.path().join(path);
            let daemon = daemon.clone();

            tokio::spawn(async move {
                let result = WalletHandle::open_or_create(
                    path.display().to_string(),
                    daemon,
                    Network::Mainnet,
                    true,
                )
                .await;

                assert!(
                    result.is_ok(),
                    "Failed to create wallet in path: `{}`",
                    path.display()
                );
            })
        })
        .collect::<Vec<_>>();

    futures::future::try_join_all(futures).await.unwrap();
}
