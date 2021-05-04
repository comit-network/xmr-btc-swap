#[tokio::test]
async fn monerod_integration_test() {
    let _client = monero_rpc::monerod::Client::localhost(18081).unwrap();
}
