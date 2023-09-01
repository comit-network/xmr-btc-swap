
use jsonrpsee::ws_client::WsClientBuilder;
use jsonrpsee_core::client::ClientT;
use jsonrpsee_core::params::ObjectParams;


use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use swap::api::request::{Method, Request };
use swap::api::Context;
use swap::cli::command::{Bitcoin, Monero};
use tokio::sync::OnceCell;

use uuid::Uuid;

#[cfg(test)]

const SERVER_ADDRESS: &str = "127.0.0.1:1234";
const BITCOIN_ADDR: &str = "tb1qr3em6k3gfnyl8r7q0v7t4tlnyxzgxma3lressv";
const MONERO_ADDR: &str = "53gEuGZUhP9JMEBZoGaFNzhwEgiG7hwQdMCqFxiyiTeFPmkbt1mAoNybEUvYBKHcnrSgxnVWgZsTvRBaHBNXPa8tHiCU51a";
const SELLER: &str =
    "/ip4/127.0.0.1/tcp/9939/p2p/12D3KooWCdMKjesXMJz1SiZ7HgotrxuqhQJbP5sgBm2BwP1cqThi";

pub async fn initialize_context() -> Arc<Context> {
    let (is_testnet, debug, json) = (true, false, false);
    let server_address = None;

    let bitcoin = Bitcoin {
        bitcoin_electrum_rpc_url: None,
        bitcoin_target_block: None,
    };

    let _monero = Monero {
        monero_daemon_address: None,
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
    )
    .await
    .unwrap();

    Arc::new(context)
}

pub async fn start_server() {
    ONCE.get_or_init(|| async {
        let ctx = initialize_context().await;
        ctx
    }).await;
    let request = Request::new(Method::StartDaemon { server_address: None });
    tokio::spawn(async move {
        request.call(Arc::clone(ONCE.get().unwrap())).await
    });
}


static ONCE: OnceCell<Arc<Context>> = OnceCell::const_new();


#[tokio::test]
pub async fn get_bitcoin_balance() {
    start_server().await;

    tokio::time::sleep(Duration::from_secs(3)).await;
    let url = format!("ws://{}", SERVER_ADDRESS);
    let mut params = ObjectParams::new();

    params.insert("", "").unwrap();

    let client = WsClientBuilder::default().build(&url).await.unwrap();
    let response: Result<HashMap<String, i32>, jsonrpsee_core::Error> = client
        .request("get_bitcoin_balance", params)
        .await;

    match response {
        Ok(_) => (),
        Err(e) => panic!("Expected a HashMap, got an error: {}", e),
    }
}

#[tokio::test]
pub async fn get_history() {
    start_server().await;


    let url = format!("ws://{}", SERVER_ADDRESS);

    tokio::time::sleep(Duration::from_secs(3)).await;
    let client = WsClientBuilder::default().build(&url).await.unwrap();
    let mut params = ObjectParams::new();
    params.insert("", "").unwrap();

    let response: Result<HashMap<String, Vec<(Uuid, String)>>, jsonrpsee_core::Error> =
        client.request("get_history", params).await;

    match response {
        Ok(_) => (),
        Err(e) => panic!("Expected a HashMap, got an error: {}", e),
    }
}

#[tokio::test]
pub async fn get_raw_history() {
    start_server().await;

    let url = format!("ws://{}", SERVER_ADDRESS);

    tokio::time::sleep(Duration::from_secs(3)).await;
    let client = WsClientBuilder::default().build(&url).await.unwrap();
    let mut params = ObjectParams::new();
    params.insert("", "").unwrap();

    let response: Result<HashMap<String, HashMap<Uuid, String>>, jsonrpsee_core::Error> =
        client.request("get_raw_history", params).await;

    match response {
        Ok(_) => (),
        Err(e) => panic!("Expected a HashMap, got an error: {}", e),
    }
}

#[tokio::test]
pub async fn get_swap_info() {
    start_server().await;

    let url = format!("ws://{}", SERVER_ADDRESS);

    tokio::time::sleep(Duration::from_secs(3)).await;
    let client = WsClientBuilder::default().build(&url).await.unwrap();
    let mut params = ObjectParams::new();
    params.insert("", "").unwrap();

    let response: Result<HashMap<String, String>, jsonrpsee_core::Error> =
        client.request("get_swap_info", params).await;

    match response {
        Err(_) => (),
        _ => panic!("Expected an error when swap_id is missing"),
    }

    let mut params = ObjectParams::new();
    params.insert("swap_id", "invalid_swap").unwrap();

    let response: Result<HashMap<String, String>, jsonrpsee_core::Error> =
        client.request("get_swap_info", params).await;

    match response {
        Err(_) => (),
        _ => panic!("Expected an error when swap_id is malformed"),
    }
}

#[tokio::test]
pub async fn withdraw_btc() {
    start_server().await;

    let url = format!("ws://{}", SERVER_ADDRESS);

    tokio::time::sleep(Duration::from_secs(3)).await;
    let client = WsClientBuilder::default().build(&url).await.unwrap();
    let params = ObjectParams::new();

    let response: Result<HashMap<String, String>, jsonrpsee_core::Error> = client.request("withdraw_btc", params).await;

    match response {
        Err(_) => (),
        _ => panic!("Expected an error when withdraw_address is missing"),
    }

    let mut params = ObjectParams::new();
    params.insert("address", "invalid_address").unwrap();

    let response: Result<HashMap<String, String>, jsonrpsee_core::Error> = client.request("withdraw_btc", params).await;

    match response {
        Err(_) => (),
        _ => panic!("Expected an error when withdraw_address is malformed"),
    }

    let mut params = ObjectParams::new();
    params.insert("address", BITCOIN_ADDR).unwrap();
    params.insert("amount", "0").unwrap();

    let response: Result<HashMap<String, String>, jsonrpsee_core::Error> = client.request("withdraw_btc", params).await;

    match response {
        Err(_) => (),
        _ => panic!("Expected an error when amount is 0"),
    }

}

#[tokio::test]
pub async fn buy_xmr() {
    start_server().await;

    let url = format!("ws://{}", SERVER_ADDRESS);

    tokio::time::sleep(Duration::from_secs(3)).await;
    let client = WsClientBuilder::default().build(&url).await.unwrap();
    let params = ObjectParams::new();

    let response: Result<HashMap<String, String>, jsonrpsee_core::Error> = client.request("buy_xmr", params).await;

    match response {
        Err(_) => (),
        _ => panic!("Expected an error when no params are given"),
    }

    let mut params = ObjectParams::new();
    params.insert("bitcoin_change_address", BITCOIN_ADDR).unwrap();
    params.insert("monero_receive_address", MONERO_ADDR).unwrap();

    let response: Result<HashMap<String, String>, jsonrpsee_core::Error> = client.request("buy_xmr", params).await;

    match response {
        Err(_) => (),
        _ => panic!("Expected an error when seller is missing"),
    }

    let mut params = ObjectParams::new();
    params.insert("bitcoin_change_address", BITCOIN_ADDR).unwrap();
    params.insert("seller", SELLER).unwrap();

    let response: Result<HashMap<String, String>, jsonrpsee_core::Error> = client.request("buy_xmr", params).await;

    match response {
        Err(_) => (),
        _ => panic!("Expected an error when monero_receive_address is missing"),
    }

    let mut params = ObjectParams::new();
    params.insert("monero_receive_address", MONERO_ADDR).unwrap();
    params.insert("seller", SELLER).unwrap();

    let response: Result<HashMap<String, String>, jsonrpsee_core::Error> = client.request("buy_xmr", params).await;

    match response {
        Err(_) => (),
        _ => panic!("Expected an error when bitcoin_change_address is missing"),
    }

    let mut params = ObjectParams::new();
    params.insert("bitcoin_change_address", "invalid_address").unwrap();
    params.insert("monero_receive_address", MONERO_ADDR).unwrap();
    params.insert("seller", SELLER).unwrap();

    let response: Result<HashMap<String, String>, jsonrpsee_core::Error> = client.request("buy_xmr", params).await;

    match response {
        Err(_) => (),
        _ => panic!("Expected an error when bitcoin_change_address is malformed"),
    }

    let mut params = ObjectParams::new();
    params.insert("bitcoin_change_address", BITCOIN_ADDR).unwrap();
    params.insert("monero_receive_address", "invalid_address").unwrap();
    params.insert("seller", SELLER).unwrap();

    let response: Result<HashMap<String, String>, jsonrpsee_core::Error> = client.request("buy_xmr", params).await;

    match response {
        Err(_) => (),
        _ => panic!("Expected an error when monero_receive_address is malformed"),
    }

    let mut params = ObjectParams::new();
    params.insert("bitcoin_change_address", BITCOIN_ADDR).unwrap();
    params.insert("monero_receive_address", MONERO_ADDR).unwrap();
    params.insert("seller", "invalid_seller").unwrap();

    let response: Result<HashMap<String, String>, jsonrpsee_core::Error> = client.request("buy_xmr", params).await;

    match response {
        Err(_) => (),
        _ => panic!("Expected an error when seller is malformed"),
    }

    let mut params = ObjectParams::new();
    params.insert("bitcoin_change_address", BITCOIN_ADDR).unwrap();
    params.insert("monero_receive_address", MONERO_ADDR).unwrap();
    params.insert("seller", SELLER).unwrap();

    let response: Result<HashMap<String, String>, jsonrpsee_core::Error> = client.request("buy_xmr", params).await;

    match response {
        Ok(_) => (),
        Err(e) => panic!("Expected a HashMap, got an error: {}", e),
    }

}
