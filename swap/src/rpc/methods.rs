use jsonrpsee::http_server::{RpcModule};
use crate::api::{Request, Init, Params};
use crate::env::{Config, GetConfig, Testnet};
use crate::fs::system_data_dir;
use url::Url;
use crate::cli::command::{Command, Options};
use std::str::FromStr;
use crate::cli::command::{DEFAULT_ELECTRUM_RPC_URL_TESTNET, DEFAULT_BITCOIN_CONFIRMATION_TARGET_TESTNET};
use crate::rpc::Error;
use crate::{bitcoin, cli, monero};
use std::sync::Arc;
use serde_json::json;
use uuid::Uuid;
use std::collections::HashMap;
use libp2p::core::Multiaddr;

pub fn register_modules(context: Arc<Init>) -> RpcModule<Arc<Init>> {
    let mut module = RpcModule::new(context);
    module
        .register_async_method("get_bitcoin_balance", |_, context| async move {
            get_bitcoin_balance(&context).await.map_err(|err| jsonrpsee_core::Error::Custom(err.to_string()))
        },
        )
        .unwrap();
    module
        .register_async_method("get_history", |_, context| async move {
            get_history(&context).await.map_err(|err| jsonrpsee_core::Error::Custom(err.to_string()))
        },
        )
        .unwrap();
    module
        .register_async_method("resume_swap", |params, context| async move {
            let swap_id: HashMap<String, String> = params.parse()?;
            let swap_id = Uuid::from_str(swap_id.get("swap_id").ok_or_else(|| jsonrpsee_core::Error::Custom("Does not contain swap_id".to_string()))?).unwrap();
            resume_swap(swap_id, &context).await.map_err(|err| jsonrpsee_core::Error::Custom(err.to_string()))
        },
        )
        .unwrap();
    module
        .register_async_method("withdraw_btc", |params, context| async move {
            let map_params: HashMap<String, String> = params.parse()?;
            let amount = if let Some(amount_str) = map_params.get("amount") {
                Some(::bitcoin::Amount::from_str_in(amount_str, ::bitcoin::Denomination::Bitcoin).map_err(|err| jsonrpsee_core::Error::Custom("Unable to parse amount".to_string()))?)
            } else {
                None
            };
            let withdraw_address = bitcoin::Address::from_str(map_params.get("address").ok_or_else(|| jsonrpsee_core::Error::Custom("Does not contain address".to_string()))?).unwrap();
            withdraw_btc(withdraw_address, amount, &context).await.map_err(|err| jsonrpsee_core::Error::Custom(err.to_string()))
        },
        )
        .unwrap();
    module
        .register_async_method("buy_xmr", |params, context| async move {
            let map_params: HashMap<String, String> = params.parse()?;
            let bitcoin_change_address = bitcoin::Address::from_str(map_params.get("bitcoin_change_address").ok_or_else(|| jsonrpsee_core::Error::Custom("Does not contain bitcoin_change_address".to_string()))?).unwrap();
            let monero_receive_address = monero::Address::from_str(map_params.get("monero_receive_address").ok_or_else(|| jsonrpsee_core::Error::Custom("Does not contain monero_receiveaddress".to_string()))?).unwrap();
            let seller = Multiaddr::from_str(map_params.get("seller").ok_or_else(|| jsonrpsee_core::Error::Custom("Does not contain seller".to_string()))?).unwrap();
            buy_xmr(bitcoin_change_address, monero_receive_address, seller, &context).await.map_err(|err| jsonrpsee_core::Error::Custom(err.to_string()))
        },
        )
        .unwrap();
    module
        .register_async_method("list_sellers", |params, context| async move {
            let map_params: HashMap<String, String> = params.parse()?;
            let rendezvous_point = Multiaddr::from_str(map_params.get("rendezvous_point").ok_or_else(|| jsonrpsee_core::Error::Custom("Does not contain rendezvous_point".to_string()))?).unwrap();
            list_sellers(rendezvous_point, &context).await.map_err(|err| jsonrpsee_core::Error::Custom(err.to_string()))
        },
        )
        .unwrap();
    module
}

async fn get_bitcoin_balance(context: &Arc<Arc<Init>>) -> anyhow::Result<serde_json::Value, Error> {
    let request = Request {
        params: Params::default(),
        cmd: Command::Balance,
    };
    let balance = request.call(Arc::clone(context)).await.unwrap();
    Ok(balance)
}

async fn get_history(context: &Arc<Arc<Init>>) -> anyhow::Result<serde_json::Value, Error> {
    let request = Request {
        params: Params::default(),
        cmd: Command::History,
    };
    let history = request.call(Arc::clone(context)).await.unwrap();
    Ok(history)
}

async fn resume_swap(swap_id: Uuid, context: &Arc<Arc<Init>>) -> anyhow::Result<serde_json::Value, Error> {
    let request = Request {
        params: Params {
            swap_id: Some(swap_id),
            ..Default::default()
        },
        cmd: Command::Resume,
    };

    let result = request.call(Arc::clone(context)).await.unwrap();
    Ok(result)
}
async fn withdraw_btc(withdraw_address: bitcoin::Address, amount: Option<bitcoin::Amount>, context: &Arc<Arc<Init>>) -> anyhow::Result<serde_json::Value, Error> {
    let request = Request {
        params: Params {
            amount: amount,
            address: Some(withdraw_address),
            ..Default::default()
        },
        cmd: Command::WithdrawBtc,
    };
    let result = request.call(Arc::clone(context)).await.unwrap();
    Ok(result)
}

async fn buy_xmr(bitcoin_change_address: bitcoin::Address, monero_receive_address: monero::Address, seller: Multiaddr, context: &Arc<Arc<Init>>) -> anyhow::Result<serde_json::Value, Error> {
    let request = Request {
        params: Params {
            bitcoin_change_address: Some(bitcoin_change_address),
            monero_receive_address: Some(monero_receive_address),
            seller: Some(seller),
            ..Default::default()
        },
        cmd: Command::BuyXmr,
    };
    let swap = request.call(Arc::clone(context)).await.unwrap();
    Ok(swap)
}

async fn list_sellers(rendezvous_point: Multiaddr, context: &Arc<Arc<Init>>) -> anyhow::Result<serde_json::Value, Error> {
    let request = Request {
        params: Params {
            rendezvous_point: Some(rendezvous_point),
            ..Default::default()
        },
        cmd: Command::ListSellers,
    };
    let result = request.call(Arc::clone(context)).await.unwrap();
    Ok(result)
}
