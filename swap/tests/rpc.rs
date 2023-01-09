use testcontainers::clients::Cli;
use testcontainers::{Container, Docker, RunArgs};
use anyhow::{bail, Context as AnyContext, Result};
use futures::Future;
use swap::api::{Context, Config};
use swap::api::request::{Request, Params, Method, Shutdown};
use std::sync::Arc;
use tokio::time::{interval, timeout};
use std::time::Duration;
use jsonrpsee::ws_client::WsClientBuilder;
use jsonrpsee_core::{client::ClientT, params::ObjectParams};
use jsonrpsee::{rpc_params, RpcModule};
use jsonrpsee_types::error::CallError;
use tokio::sync::broadcast;
use swap::cli::command::{Bitcoin, Monero};
use std::collections::HashMap;
use uuid::Uuid;
use serde_json::{json, Value};
use sequential_test::sequential;

#[cfg(test)]


// to be replaced with actual "real" testing values
// need to create some kind of swap database and bitcoin environment with some funds
const SERVER_ADDRESS: &str = "127.0.0.1:1234";
const BITCOIN_ADDR: &str = "valid_address";
const MONERO_ADDR: &str = "valid_address";
const SELLER: &str = "some_seller";
const SWAP_ID: &str = "valid_swap_id";

pub async fn initialize_context() -> (Arc<Context>, Request) {
    let (is_testnet, debug, json) = (true, false, false);
    //let data_dir = data::data_dir_from(None, is_testnet).unwrap();
    let server_address = None;
    let (tx, _) = broadcast::channel(1);

    let bitcoin = Bitcoin {
        bitcoin_electrum_rpc_url: None,
        bitcoin_target_block: None,
    };

    let monero = Monero {
        monero_daemon_address: None,
    };

    let mut request = Request {
        params: Params::default(),
        cmd: Method::StartDaemon,
        shutdown: Shutdown::new(tx.subscribe()),
    };

    let context = Context::build(
        Some(bitcoin),
        None,
        None,
        None,
        is_testnet,
        debug,
        json,
        server_address,
        tx,
    ).await.unwrap();

    (Arc::new(context), request)
}


#[tokio::test]
#[sequential]
pub async fn can_start_server() {
    let (ctx, mut request) = initialize_context().await;
    let move_ctx = Arc::clone(&ctx);
    tokio::spawn(async move {
        request.call(Arc::clone(&move_ctx)).await;
    });
    tokio::time::sleep(Duration::from_secs(3)).await;
    ctx.shutdown.send(());
    assert!(true);
}

#[tokio::test]
#[sequential]
pub async fn get_bitcoin_balance() {
    let (ctx, mut request) = initialize_context().await;
    let move_ctx = Arc::clone(&ctx);
    tokio::spawn(async move {
        request.call(Arc::clone(&move_ctx)).await;
    });

    let url = format!("ws://{}", SERVER_ADDRESS);
    tokio::time::sleep(Duration::from_secs(3)).await;

    let client = WsClientBuilder::default().build(&url).await.unwrap();
	let response: HashMap<String, i32> = client.request("get_bitcoin_balance", rpc_params!["id"]).await.unwrap();

    assert_eq!(response, HashMap::from([("balance".to_string(), 0)]));
    ctx.shutdown.send(());
}

#[tokio::test]
#[sequential]
pub async fn get_history() {
    let (ctx, mut request) = initialize_context().await;
    let move_ctx = Arc::clone(&ctx);
    tokio::spawn(async move {
        request.call(Arc::clone(&move_ctx)).await;
    });

    let url = format!("ws://{}", SERVER_ADDRESS);
    tokio::time::sleep(Duration::from_secs(3)).await;

    let client = WsClientBuilder::default().build(&url).await.unwrap();
    let mut params = ObjectParams::new();

	let response: HashMap<String, Vec<(Uuid, String)>> = client.request("get_history", params).await.unwrap();
    let swaps: Vec<(Uuid, String)> = Vec::new();

    assert_eq!(response, HashMap::from([("swaps".to_string(), swaps)]));
    ctx.shutdown.send(());
}

#[tokio::test]
#[sequential]
pub async fn get_raw_history() {
    let (ctx, mut request) = initialize_context().await;
    let move_ctx = Arc::clone(&ctx);
    tokio::spawn(async move {
        request.call(Arc::clone(&move_ctx)).await;
    });

    let url = format!("ws://{}", SERVER_ADDRESS);
    tokio::time::sleep(Duration::from_secs(3)).await;

    let client = WsClientBuilder::default().build(&url).await.unwrap();
    let mut params = ObjectParams::new();
    let raw_history: HashMap<Uuid, String> = HashMap::new();

	let response: HashMap<String, HashMap<Uuid, String>> = client.request("get_raw_history", params).await.unwrap();

    assert_eq!(response, HashMap::from([("raw_history".to_string(), raw_history)]));
    ctx.shutdown.send(());
}

#[tokio::test]
#[sequential]
pub async fn get_seller() {
    let (ctx, mut request) = initialize_context().await;
    let move_ctx = Arc::clone(&ctx);
    tokio::spawn(async move {
        request.call(Arc::clone(&move_ctx)).await;
    });

    let url = format!("ws://{}", SERVER_ADDRESS);
    tokio::time::sleep(Duration::from_secs(3)).await;

    let client = WsClientBuilder::default().build(&url).await.unwrap();
    let mut params = ObjectParams::new();

    let response: Result<HashMap<String, String>, _> = client.request("get_seller", params).await;

    // We should ideally match the expected error and panic if it's different one, but the request returns a custom error (to investigate)
    // Err(jsonrpsee_core::Error::Call(CallError::InvalidParams(e))) => (),
    // Err(e) => panic!("ErrorType was not ParseError but {e:?}"),

    match response {
        Err(e) => (),
        _ => panic!("Expected an error when swap_id is missing"),
    }

    let mut params = ObjectParams::new();
    params.insert("swap_id", "invalid_swap");

    let response: Result<HashMap<String, String>, _> = client.request("get_seller", params).await;

    match response {
        Err(e) => (),
        _ => panic!("Expected an error swap_id is malformed"),
    }

    let mut params = ObjectParams::new();
    params.insert("swap_id", SWAP_ID);

    let response: Result<HashMap<String, String>, _> = client.request("get_seller", params).await;

    match response {
        Ok(hash) => (),
        Err(e) => panic!("Expected a HashMap with correct params, got an error: {}", e),
    }
    ctx.shutdown.send(());
}

#[tokio::test]
#[sequential]
pub async fn get_swap_start_date() {
    let (ctx, mut request) = initialize_context().await;
    let move_ctx = Arc::clone(&ctx);
    tokio::spawn(async move {
        request.call(Arc::clone(&move_ctx)).await;
    });

    let url = format!("ws://{}", SERVER_ADDRESS);
    tokio::time::sleep(Duration::from_secs(3)).await;

    let client = WsClientBuilder::default().build(&url).await.unwrap();
    let mut params = ObjectParams::new();

    let response: Result<HashMap<String, String>, _> = client.request("get_swap_start_date", params).await;

    match response {
        Err(e) => (),
        _ => panic!("Expected an error when swap_id is missing"),
    }

    let mut params = ObjectParams::new();
    params.insert("swap_id", "invalid_swap");

    let response: Result<HashMap<String, String>, _> = client.request("get_swap_start_date", params).await;

    match response {
        Err(e) => (),
        _ => panic!("Expected an error when swap_id is malformed"),
    }

    let mut params = ObjectParams::new();
    params.insert("swap_id", SWAP_ID);

    let response: Result<HashMap<String, String>, _> = client.request("get_swap_start_date", params).await;

    match response {
        Ok(hash) => (),
        Err(e) => panic!("Expected a HashMap, got an error: {}", e),
    }
    ctx.shutdown.send(());
}

#[tokio::test]
#[sequential]
pub async fn resume_swap() {
    let (ctx, mut request) = initialize_context().await;
    let move_ctx = Arc::clone(&ctx);
    tokio::spawn(async move {
        request.call(Arc::clone(&move_ctx)).await;
    });

    let url = format!("ws://{}", SERVER_ADDRESS);
    tokio::time::sleep(Duration::from_secs(3)).await;

    let client = WsClientBuilder::default().build(&url).await.unwrap();
    let mut params = ObjectParams::new();

    let response: Result<HashMap<String, String>, _> = client.request("get_swap_start_date", params).await;

    match response {
        Err(e) => (),
        _ => panic!("Expected an error when swap_id is missing"),
    }

    let mut params = ObjectParams::new();
    params.insert("swap_id", "invalid_swap");

    let response: Result<HashMap<String, String>, _> = client.request("get_swap_start_date", params).await;

    match response {
        Err(e) => (),
        _ => panic!("Expected an error when swap_id is malformed"),
    }

    let mut params = ObjectParams::new();
    params.insert("swap_id", SWAP_ID);

    let response: Result<HashMap<String, String>, _> = client.request("get_swap_start_date", params).await;

    match response {
        Ok(hash) => (),
        Err(e) => panic!("Expected a HashMap, got an error: {}", e),
    }
    ctx.shutdown.send(());
}

#[tokio::test]
#[sequential]
pub async fn withdraw_btc() {
    let (ctx, mut request) = initialize_context().await;
    let move_ctx = Arc::clone(&ctx);
    tokio::spawn(async move {
        request.call(Arc::clone(&move_ctx)).await;
    });

    let url = format!("ws://{}", SERVER_ADDRESS);
    tokio::time::sleep(Duration::from_secs(3)).await;

    let client = WsClientBuilder::default().build(&url).await.unwrap();
    let mut params = ObjectParams::new();

    let response: Result<HashMap<String, String>, _> = client.request("withdraw_btc", params).await;

    match response {
        Err(e) => (),
        _ => panic!("Expected an error when withdraw_address is missing"),
    }

    let mut params = ObjectParams::new();
    params.insert("address", "invalid_address");

    let response: Result<HashMap<String, String>, _> = client.request("withdraw_btc", params).await;

    match response {
        Err(e) => (),
        _ => panic!("Expected an error when withdraw_address is malformed"),
    }

    let mut params = ObjectParams::new();
    params.insert("address", BITCOIN_ADDR);
    params.insert("amount", "0");

    let response: Result<HashMap<String, String>, _> = client.request("withdraw_btc", params).await;

    match response {
        Err(e) => (),
        _ => panic!("Expected an error when amount is 0"),
    }

    let mut params = ObjectParams::new();
    params.insert("address", BITCOIN_ADDR);
    params.insert("amount", "0.1");

    let response: Result<HashMap<String, String>, _> = client.request("withdraw_btc", params).await;

    match response {
        Ok(hash) => (),
        Err(e) => panic!("Expected a HashMap, got an error: {}", e),
    }

    ctx.shutdown.send(());
}

#[tokio::test]
#[sequential]
pub async fn buy_xmr() {
    let (ctx, mut request) = initialize_context().await;
    let move_ctx = Arc::clone(&ctx);
    tokio::spawn(async move {
        request.call(Arc::clone(&move_ctx)).await;
    });

    let url = format!("ws://{}", SERVER_ADDRESS);
    tokio::time::sleep(Duration::from_secs(3)).await;

    let client = WsClientBuilder::default().build(&url).await.unwrap();
    let mut params = ObjectParams::new();

    let response: Result<HashMap<String, String>, _> = client.request("buy_xmr", params).await;

    match response {
        Err(e) => (),
        _ => panic!("Expected an error when no params are given"),
    }

    let mut params = ObjectParams::new();
    params.insert("bitcoin_change_address", BITCOIN_ADDR);
    params.insert("monero_receive_address", MONERO_ADDR);

    let response: Result<HashMap<String, String>, _> = client.request("buy_xmr", params).await;

    match response {
        Err(e) => (),
        _ => panic!("Expected an error when seller is missing"),
    }

    let mut params = ObjectParams::new();
    params.insert("bitcoin_change_address", BITCOIN_ADDR);
    params.insert("seller", SELLER);

    let response: Result<HashMap<String, String>, _> = client.request("buy_xmr", params).await;

    match response {
        Err(e) => (),
        _ => panic!("Expected an error when monero_receive_address is missing"),
    }

    let mut params = ObjectParams::new();
    params.insert("monero_receive_address", MONERO_ADDR);
    params.insert("seller", SELLER);

    let response: Result<HashMap<String, String>, _> = client.request("buy_xmr", params).await;

    match response {
        Err(e) => (),
        _ => panic!("Expected an error when bitcoin_change_address is missing"),
    }

    let mut params = ObjectParams::new();
    params.insert("bitcoin_change_address", "invalid_address");
    params.insert("monero_receive_address", MONERO_ADDR);
    params.insert("seller", SELLER);

    let response: Result<HashMap<String, String>, _> = client.request("buy_xmr", params).await;

    match response {
        Err(e) => (),
        _ => panic!("Expected an error when bitcoin_change_address is malformed"),
    }

    let mut params = ObjectParams::new();
    params.insert("bitcoin_change_address", BITCOIN_ADDR);
    params.insert("monero_receive_address", "invalid_address");
    params.insert("seller", SELLER);

    let response: Result<HashMap<String, String>, _> = client.request("buy_xmr", params).await;

    match response {
        Err(e) => (),
        _ => panic!("Expected an error when monero_receive_address is malformed"),
    }

    let mut params = ObjectParams::new();
    params.insert("bitcoin_change_address", BITCOIN_ADDR);
    params.insert("monero_receive_address", MONERO_ADDR);
    params.insert("seller", "invalid_seller");

    let response: Result<HashMap<String, String>, _> = client.request("buy_xmr", params).await;

    match response {
        Err(e) => (),
        _ => panic!("Expected an error when seller is malformed"),
    }

    let mut params = ObjectParams::new();
    params.insert("bitcoin_change_address", BITCOIN_ADDR);
    params.insert("monero_receive_address", MONERO_ADDR);
    params.insert("seller", SELLER);

    let response: Result<HashMap<String, String>, _> = client.request("buy_xmr", params).await;

    match response {
        Ok(hash) => (),
        Err(e) => panic!("Expected a HashMap, got an error: {}", e),
    }

    ctx.shutdown.send(());
}
