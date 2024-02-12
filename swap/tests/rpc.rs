pub mod harness;
#[cfg(test)]
mod test {

    use anyhow::Result;

    use jsonrpsee::ws_client::WsClientBuilder;
    use jsonrpsee_core::client::{Client, ClientT};
    use jsonrpsee_core::params::ObjectParams;

    use serial_test::serial;

    use serde_json::Value;
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::time::Duration;
    use swap::api::request::{Method, Request};
    use swap::api::Context;

    use crate::harness::alice_run_until::is_xmr_lock_transaction_sent;
    use crate::harness::bob_run_until::is_btc_locked;
    use crate::harness::{setup_test, SlowCancelConfig, TestContext};
    use swap::asb::FixedRate;
    use swap::protocol::{alice, bob};
    use swap::tracing_ext::{capture_logs, MakeCapturingWriter};
    use tracing_subscriber::filter::LevelFilter;
    use uuid::Uuid;

    const SERVER_ADDRESS: &str = "127.0.0.1:1234";
    const SERVER_START_TIMEOUT_SECS: u64 = 50;
    const BITCOIN_ADDR: &str = "bcrt1qahvhjfc7vx5857zf8knxs8yp5lkm26jgyt0k76";
    const MONERO_ADDR: &str = "53gEuGZUhP9JMEBZoGaFNzhwEgiG7hwQdMCqFxiyiTeFPmkbt1mAoNybEUvYBKHcnrSgxnVWgZsTvRBaHBNXPa8tHiCU51a";
    const SELLER: &str =
        "/ip4/127.0.0.1/tcp/9939/p2p/12D3KooWCdMKjesXMJz1SiZ7HgotrxuqhQJbP5sgBm2BwP1cqThi";
    const SWAP_ID: &str = "ea030832-3be9-454f-bb98-5ea9a788406b";

    pub async fn setup_daemon(
        harness_ctx: TestContext,
    ) -> (Client, MakeCapturingWriter, Arc<Context>) {
        let writer = capture_logs(LevelFilter::DEBUG);
        let server_address: SocketAddr = SERVER_ADDRESS.parse().unwrap();

        let request = Request::new(Method::StartDaemon {
            server_address: Some(server_address),
        });

        let context = Arc::new(harness_ctx.get_bob_context().await);

        let context_clone = context.clone();

        tokio::spawn(async move {
            if let Err(err) = request.call(context_clone).await {
                println!("Failed to initialize daemon for testing: {}", err);
            }
        });

        for _ in 0..SERVER_START_TIMEOUT_SECS {
            if writer.captured().contains("Started RPC server") {
                let url = format!("ws://{}", SERVER_ADDRESS);
                let client = WsClientBuilder::default().build(&url).await.unwrap();

                return (client, writer, context);
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        panic!(
            "Failed to start RPC server after {} seconds",
            SERVER_START_TIMEOUT_SECS
        );
    }

    fn assert_has_keys_serde(map: &serde_json::Map<String, Value>, keys: &[&str]) {
        for &key in keys {
            assert!(map.contains_key(key), "Key {} is missing", key);
        }
    }

    // Helper function for HashMap
    fn assert_has_keys_hashmap<T>(map: &HashMap<String, T>, keys: &[&str]) {
        for &key in keys {
            assert!(map.contains_key(key), "Key {} is missing", key);
        }
    }

    #[tokio::test]
    #[serial]
    pub async fn get_swap_info() {
        setup_test(SlowCancelConfig, |mut harness_ctx| async move {
            // Start a swap and wait for xmr lock transaction to be published (XmrLockTransactionSent)
            let (bob_swap, _) = harness_ctx.bob_swap().await;
            let bob_swap_id = bob_swap.id;
            tokio::spawn(bob::run_until(bob_swap, is_btc_locked));
            let alice_swap = harness_ctx.alice_next_swap().await;
            alice::run_until(
                alice_swap,
                is_xmr_lock_transaction_sent,
                FixedRate::default(),
            )
            .await?;

            let (client, _, _) = setup_daemon(harness_ctx).await;

            let response: HashMap<String, Vec<(Uuid, String)>> = client
                .request("get_history", ObjectParams::new())
                .await
                .unwrap();
            let swaps: Vec<(Uuid, String)> = vec![(bob_swap_id, "btc is locked".to_string())];

            assert_eq!(response, HashMap::from([("swaps".to_string(), swaps)]));

            let response: HashMap<String, HashMap<Uuid, Vec<Value>>> = client
                .request("get_raw_states", ObjectParams::new())
                .await
                .unwrap();

            let response_raw_states = response.get("raw_states").unwrap();

            assert!(response_raw_states.contains_key(&bob_swap_id));
            assert_eq!(response_raw_states.get(&bob_swap_id).unwrap().len(), 2);

            let mut params = ObjectParams::new();
            params.insert("swap_id", bob_swap_id).unwrap();
            let response: HashMap<String, Value> =
                client.request("get_swap_info", params).await.unwrap();

            // Check primary keys in response
            assert_has_keys_hashmap(
                &response,
                &[
                    "txRefundFee",
                    "swapId",
                    "cancelTimelock",
                    "timelock",
                    "punishTimelock",
                    "stateName",
                    "btcAmount",
                    "startDate",
                    "btcRefundAddress",
                    "txCancelFee",
                    "xmrAmount",
                    "completed",
                    "txLockId",
                    "seller",
                ],
            );

            // Assert specific fields
            assert_eq!(response.get("swapId").unwrap(), &bob_swap_id.to_string());
            assert_eq!(
                response.get("stateName").unwrap(),
                &"btc is locked".to_string()
            );
            assert_eq!(response.get("completed").unwrap(), &Value::Bool(false));

            // Check seller object and its keys
            let seller = response
                .get("seller")
                .expect("Field 'seller' is missing from response")
                .as_object()
                .expect("'seller' is not an object");
            assert_has_keys_serde(seller, &["peerId"]);

            // Check timelock object, nested 'None' object, and blocks_left
            let timelock = response
                .get("timelock")
                .expect("Field 'timelock' is missing from response")
                .as_object()
                .expect("'timelock' is not an object");
            let none_obj = timelock
                .get("None")
                .expect("Field 'None' is missing from 'timelock'")
                .as_object()
                .expect("'None' is not an object in 'timelock'");
            let blocks_left = none_obj
                .get("blocks_left")
                .expect("Field 'blocks_left' is missing from 'None'")
                .as_i64()
                .expect("'blocks_left' is not an integer");

            // Validate blocks_left
            assert!(
                blocks_left > 0 && blocks_left <= 180,
                "Field 'blocks_left' should be > 0 and <= 180 but got {}",
                blocks_left
            );

            Ok(())
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    pub async fn test_rpc_calls() {
        setup_test(SlowCancelConfig, |harness_ctx| async move {
            let alice_addr = harness_ctx.bob_params.get_concentenated_alice_address();
            let (change_address, receive_address) =
                harness_ctx.bob_params.get_change_receive_addresses().await;

            let (client, writer, _) = setup_daemon(harness_ctx).await;
            assert!(client.is_connected());

            let mut params = ObjectParams::new();

            params.insert("force_refresh", false).unwrap();
            let response: HashMap<String, i32> = client
                .request("get_bitcoin_balance", params)
                .await
                .unwrap();

            assert_eq!(response, HashMap::from([("balance".to_string(), 10000000)]));


            let mut params = ObjectParams::new();
            params.insert("log_reference_id", "test_ref_id").unwrap();
            params.insert("force_refresh", false).unwrap();

            let _: HashMap<String, i32> = client.request("get_bitcoin_balance", params).await.unwrap();

            assert!(writer.captured().contains(
                r#"method{method_name="Balance" log_reference_id="\"test_ref_id\""}: swap::api::request: Current Bitcoin balance as of last sync balance=0.1 BTC"#
            ));

            for method in ["get_swap_info", "resume_swap", "cancel_refund_swap"].iter() {
                let mut params = ObjectParams::new();
                params.insert("swap_id", "invalid_swap").unwrap();

                let response: Result<HashMap<String, String>, _> =
                    client.request(method, params).await;
                response.expect_err(&format!(
                    "Expected an error when swap_id is invalid for method {}",
                    method
                ));

                let params = ObjectParams::new();

                let response: Result<HashMap<String, String>, _> =
                    client.request(method, params).await;
                response.expect_err(&format!(
                    "Expected an error when swap_id is missing for method {}",
                    method
                ));
            }

            let params = ObjectParams::new();
            let result: Result<HashMap<String, String>, _> =
                client.request("list_sellers", params).await;

            result.expect_err("Expected an error when rendezvous_point is missing");

            let params = ObjectParams::new();
            let result: Result<HashMap<String, String>, _> =
                client.request("list_sellers", params).await;

            result.expect_err("Expected an error when rendezvous_point is missing");

            let params = ObjectParams::new();
            let response: Result<HashMap<String, String>, _> =
                client.request("withdraw_btc", params).await;
            response.expect_err("Expected an error when withdraw_address is missing");

            let mut params = ObjectParams::new();
            params.insert("address", "invalid_address").unwrap();
            let response: Result<HashMap<String, String>, _> =
                client.request("withdraw_btc", params).await;
            response.expect_err("Expected an error when withdraw_address is malformed");

            let mut params = ObjectParams::new();
            params.insert("address", BITCOIN_ADDR).unwrap();
            params.insert("amount", "0").unwrap();
            let response: Result<HashMap<String, String>, _> =
                client.request("withdraw_btc", params).await;
            response.expect_err("Expected an error when amount is 0");

            let mut params = ObjectParams::new();
            params
                .insert("address", BITCOIN_ADDR)
                .unwrap();
            params.insert("amount", "0.01").unwrap();
            let response: HashMap<String, Value> = client
                .request("withdraw_btc", params)
                .await
                .expect("Expected a valid response");

            assert_has_keys_hashmap(&response, &["signed_tx", "amount", "txid"]);
            assert_eq!(
                response.get("amount").unwrap().as_u64().unwrap(),
                1_000_000
            );

            let params = ObjectParams::new();
            let response: Result<HashMap<String, String>, _> =
                client.request("buy_xmr", params).await;
            response.expect_err("Expected an error when no params are given");

            let mut params = ObjectParams::new();
            params
                .insert("bitcoin_change_address", BITCOIN_ADDR)
                .unwrap();
            params
                .insert("monero_receive_address", MONERO_ADDR)
                .unwrap();
            let response: Result<HashMap<String, String>, _> =
                client.request("buy_xmr", params).await;
            response.expect_err("Expected an error when seller is missing");

            let mut params = ObjectParams::new();
            params
                .insert("bitcoin_change_address", BITCOIN_ADDR)
                .unwrap();
            params.insert("seller", SELLER).unwrap();
            let response: Result<HashMap<String, String>, _> =
                client.request("buy_xmr", params).await;
            response.expect_err("Expected an error when monero_receive_address is missing");

            let mut params = ObjectParams::new();
            params
                .insert("monero_receive_address", MONERO_ADDR)
                .unwrap();
            params.insert("seller", SELLER).unwrap();
            let response: Result<HashMap<String, String>, _> =
                client.request("buy_xmr", params).await;
            response.expect_err("Expected an error when bitcoin_change_address is missing");

            let mut params = ObjectParams::new();
            params
                .insert("bitcoin_change_address", "invalid_address")
                .unwrap();
            params
                .insert("monero_receive_address", MONERO_ADDR)
                .unwrap();
            params.insert("seller", SELLER).unwrap();
            let response: Result<HashMap<String, String>, _> =
                client.request("buy_xmr", params).await;
            response.expect_err("Expected an error when bitcoin_change_address is malformed");

            let mut params = ObjectParams::new();
            params
                .insert("bitcoin_change_address", BITCOIN_ADDR)
                .unwrap();
            params
                .insert("monero_receive_address", "invalid_address")
                .unwrap();
            params.insert("seller", SELLER).unwrap();
            let response: Result<HashMap<String, String>, _> =
                client.request("buy_xmr", params).await;
            response.expect_err("Expected an error when monero_receive_address is malformed");


            let mut params = ObjectParams::new();
            params
                .insert("bitcoin_change_address", BITCOIN_ADDR)
                .unwrap();
            params
                .insert("monero_receive_address", MONERO_ADDR)
                .unwrap();
            params.insert("seller", "invalid_seller").unwrap();
            let response: Result<HashMap<String, String>, _> =
                client.request("buy_xmr", params).await;
            response.expect_err("Expected an error when seller is malformed");

            let response: Result<HashMap<String, String>, _> = client
                .request("suspend_current_swap", ObjectParams::new())
                .await;
            response.expect_err("Expected an error when no swap is running");

            let mut params = ObjectParams::new();
            params
                .insert("bitcoin_change_address", change_address)
                .unwrap();
            params
                .insert("monero_receive_address", receive_address)
                .unwrap();

            params.insert("seller", alice_addr).unwrap();
            let response: HashMap<String, Value> = client
                .request("buy_xmr", params)
                .await
                .expect("Expected a HashMap, got an error");

            assert_has_keys_hashmap(&response, &["swapId"]);

            Ok(())
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    pub async fn suspend_current_swap_swap_running() {
        setup_test(SlowCancelConfig, |harness_ctx| async move {
            let (client, _, ctx) = setup_daemon(harness_ctx).await;

            ctx.swap_lock
                .acquire_swap_lock(Uuid::parse_str(SWAP_ID).unwrap())
                .await
                .unwrap();
            let cloned_ctx = ctx.clone();

            tokio::spawn(async move {
                // Immediately release lock when suspend signal is received. Mocks a running swap that is then cancelled.
                ctx.swap_lock
                    .listen_for_swap_force_suspension()
                    .await
                    .unwrap();
                ctx.swap_lock.release_swap_lock().await.unwrap();
            });

            let response: HashMap<String, String> = client
                .request("suspend_current_swap", ObjectParams::new())
                .await
                .unwrap();
            assert_eq!(
                response,
                HashMap::from([("swapId".to_string(), SWAP_ID.to_string())])
            );

            cloned_ctx
                .swap_lock
                .acquire_swap_lock(Uuid::parse_str(SWAP_ID).unwrap())
                .await
                .unwrap();

            let response: Result<HashMap<String, String>, _> = client
                .request("suspend_current_swap", ObjectParams::new())
                .await;
            response.expect_err("Expected an error when suspend signal times out");

            Ok(())
        })
        .await;
    }
}
