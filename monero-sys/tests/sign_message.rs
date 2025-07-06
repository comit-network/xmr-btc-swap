use monero_sys::{Daemon, WalletHandle};

const PLACEHOLDER_NODE: &str = "http://127.0.0.1:18081";

#[tokio::test]
async fn test_sign_message() {
    tracing_subscriber::fmt()
        .with_env_filter("info,test=debug,sign_message=trace,monero_sys=trace")
        .with_test_writer()
        .init();

    let temp_dir = tempfile::tempdir().unwrap();
    let daemon = Daemon {
        address: PLACEHOLDER_NODE.into(),
        ssl: false,
    };

    let wallet_name = "test_signing_wallet";
    let wallet_path = temp_dir.path().join(wallet_name).display().to_string();

    tracing::info!("Creating wallet for message signing test");
    let wallet = WalletHandle::open_or_create(
        wallet_path,
        daemon,
        monero::Network::Stagenet,
        false, // No background sync
    )
    .await
    .expect("Failed to create wallet");

    let main_address = wallet.main_address().await;
    tracing::info!("Wallet main address: {}", main_address);

    // Test message to sign
    let test_message = "Hello, World! This is a test message for signing.";
    
    tracing::info!("Testing message signing with spend key (default address)");
    let signature_spend = wallet
        .sign_message(test_message, None, false)
        .await
        .expect("Failed to sign message with spend key");
    
    tracing::info!("Signature with spend key: {}", signature_spend);
    assert!(!signature_spend.is_empty(), "Signature should not be empty");
    assert!(signature_spend.len() > 10, "Signature should be reasonably long");

    tracing::info!("Testing message signing with view key (default address)");
    let signature_view = wallet
        .sign_message(test_message, None, true)
        .await
        .expect("Failed to sign message with view key");
    
    tracing::info!("Signature with view key: {}", signature_view);
    assert!(!signature_view.is_empty(), "Signature should not be empty");
    assert!(signature_view.len() > 10, "Signature should be reasonably long");

    // Signatures should be different when using different keys
    assert_ne!(signature_spend, signature_view, "Spend key and view key signatures should be different");

    tracing::info!("Testing message signing with spend key (explicit address)");
    let signature_explicit = wallet
        .sign_message(test_message, Some(&main_address.to_string()), false)
        .await
        .expect("Failed to sign message with explicit address");
    
    tracing::info!("Signature with explicit address: {}", signature_explicit);
    assert!(!signature_explicit.is_empty(), "Signature should not be empty");
    
    // When using the same key and same address (main address), signatures should be the same
    assert_eq!(signature_spend, signature_explicit, "Signatures should be the same when using same key and address");

    tracing::info!("Testing empty message signing");
    let signature_empty = wallet
        .sign_message("", None, false)
        .await
        .expect("Failed to sign empty message");
    
    tracing::info!("Signature for empty message: {}", signature_empty);
    assert!(!signature_empty.is_empty(), "Signature should not be empty even for empty message");

    tracing::info!("All message signing tests passed!");
} 