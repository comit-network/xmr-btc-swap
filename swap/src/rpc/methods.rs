use crate::api::request::{Method, Request};
use crate::api::Context;
use crate::bitcoin::bitcoin_address;
use crate::monero::monero_address;
use crate::{bitcoin, monero};
use anyhow::Result;
use jsonrpsee::server::RpcModule;
use libp2p::core::Multiaddr;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

pub fn register_modules(context: Arc<Context>) -> RpcModule<Arc<Context>> {
    let mut module = RpcModule::new(context);

    module
        .register_async_method("get_bitcoin_balance", |_, context| async move {
            get_bitcoin_balance(&context).await
        })
        .unwrap();

    module
        .register_async_method("get_history", |_, context| async move {
            get_history(&context).await
        })
        .unwrap();

    module
        .register_async_method("get_raw_history", |_, context| async move {
            get_raw_history(&context).await
        })
        .unwrap();

    module
        .register_async_method("get_seller", |params, context| async move {
            let params: HashMap<String, Uuid> = params.parse()?;

            let swap_id = params.get("swap_id").ok_or_else(|| {
                jsonrpsee_core::Error::Custom("Does not contain swap_id".to_string())
            })?;

            get_seller(*swap_id, &context).await
        })
        .unwrap();

    module
        .register_async_method("get_swap_start_date", |params, context| async move {
            let params: HashMap<String, Uuid> = params.parse()?;

            let swap_id = params.get("swap_id").ok_or_else(|| {
                jsonrpsee_core::Error::Custom("Does not contain swap_id".to_string())
            })?;

            get_swap_start_date(*swap_id, &context).await
        })
        .unwrap();

    module
        .register_async_method("resume_swap", |params, context| async move {
            let params: HashMap<String, Uuid> = params.parse()?;

            let swap_id = params.get("swap_id").ok_or_else(|| {
                jsonrpsee_core::Error::Custom("Does not contain swap_id".to_string())
            })?;

            resume_swap(*swap_id, &context).await
        })
        .unwrap();

    module.register_async_method("get_swap_expired_timelock", |params, context| async move {
        let params: HashMap<String, Uuid> = params.parse()?;

        let swap_id = params.get("swap_id").ok_or_else(|| {
            jsonrpsee_core::Error::Custom("Does not contain swap_id".to_string())
        })?;

        get_swap_timelock(*swap_id, &context).await
    }).unwrap();

    module
        .register_async_method("cancel_refund_swap", |params, context| async move {
            let params: HashMap<String, Uuid> = params.parse()?;

            let swap_id = params.get("swap_id").ok_or_else(|| {
                jsonrpsee_core::Error::Custom("Does not contain swap_id".to_string())
            })?;

            cancel_and_refund_swap(*swap_id, &context).await
        })
        .unwrap();
    module
        .register_async_method("withdraw_btc", |params, context| async move {
            let params: HashMap<String, String> = params.parse()?;

            let amount = if let Some(amount_str) = params.get("amount") {
                Some(
                    ::bitcoin::Amount::from_str_in(amount_str, ::bitcoin::Denomination::Bitcoin)
                        .map_err(|_| {
                            jsonrpsee_core::Error::Custom("Unable to parse amount".to_string())
                        })?,
                )
            } else {
                None
            };

            let withdraw_address =
                bitcoin::Address::from_str(params.get("address").ok_or_else(|| {
                    jsonrpsee_core::Error::Custom("Does not contain address".to_string())
                })?)
                .map_err(|err| jsonrpsee_core::Error::Custom(err.to_string()))?;
            let withdraw_address =
                bitcoin_address::validate(withdraw_address, context.config.is_testnet)?;

            withdraw_btc(withdraw_address, amount, &context).await
        })
        .expect("Could not register RPC method withdraw_btc");
    module
        .register_async_method("buy_xmr", |params, context| async move {
            let params: HashMap<String, String> = params.parse()?;

            let bitcoin_change_address = bitcoin::Address::from_str(
                params.get("bitcoin_change_address").ok_or_else(|| {
                    jsonrpsee_core::Error::Custom(
                        "Does not contain bitcoin_change_address".to_string(),
                    )
                })?,
            )
            .map_err(|err| jsonrpsee_core::Error::Custom(err.to_string()))?;

            let bitcoin_change_address =
                bitcoin_address::validate(bitcoin_change_address, context.config.is_testnet)?;

            let monero_receive_address = monero::Address::from_str(
                params.get("monero_receive_address").ok_or_else(|| {
                    jsonrpsee_core::Error::Custom(
                        "Does not contain monero_receiveaddress".to_string(),
                    )
                })?,
            )
            .map_err(|err| jsonrpsee_core::Error::Custom(err.to_string()))?;

            let monero_receive_address =
                monero_address::validate(monero_receive_address, context.config.is_testnet)?;

            let seller = Multiaddr::from_str(params.get("seller").ok_or_else(|| {
                jsonrpsee_core::Error::Custom("Does not contain seller".to_string())
            })?)
            .map_err(|err| jsonrpsee_core::Error::Custom(err.to_string()))?;

            buy_xmr(
                bitcoin_change_address,
                monero_receive_address,
                seller,
                &context,
            )
            .await
        })
        .unwrap();
    module
        .register_async_method("list_sellers", |params, context| async move {
            let params: HashMap<String, Multiaddr> = params.parse()?;
            let rendezvous_point = params.get("rendezvous_point").ok_or_else(|| {
                jsonrpsee_core::Error::Custom("Does not contain rendezvous_point".to_string())
            })?;

            list_sellers(rendezvous_point.clone(), &context).await
        })
        .unwrap();
    module.register_async_method("get_current_swap", |_, context| async move {
        get_current_swap(&context).await
    }).unwrap();

    module
}

async fn execute_request(
    cmd: Method,
    context: &Arc<Context>,
) -> Result<serde_json::Value, jsonrpsee_core::Error> {
    let request = Request::new(context.shutdown.subscribe(), cmd);
    request
        .call(Arc::clone(context))
        .await
        .map_err(|err| jsonrpsee_core::Error::Custom(err.to_string()))
}

async fn get_current_swap(context: &Arc<Context>) -> Result<serde_json::Value, jsonrpsee_core::Error> {
    execute_request(Method::GetCurrentSwap, context).await
}

async fn get_bitcoin_balance(
    context: &Arc<Context>,
) -> Result<serde_json::Value, jsonrpsee_core::Error> {
    execute_request(Method::Balance, context).await
}

async fn get_history(context: &Arc<Context>) -> Result<serde_json::Value, jsonrpsee_core::Error> {
    execute_request(Method::History, context).await
}

async fn get_raw_history(
    context: &Arc<Context>,
) -> Result<serde_json::Value, jsonrpsee_core::Error> {
    execute_request(Method::RawHistory, context).await
}

async fn get_seller(
    swap_id: Uuid,
    context: &Arc<Context>,
) -> Result<serde_json::Value, jsonrpsee_core::Error> {
    execute_request(Method::GetSeller {
        swap_id
    }, context).await
}

async fn get_swap_start_date(
    swap_id: Uuid,
    context: &Arc<Context>,
) -> Result<serde_json::Value, jsonrpsee_core::Error> {
    execute_request(Method::SwapStartDate {
        swap_id
    }, context).await
}

async fn resume_swap(
    swap_id: Uuid,
    context: &Arc<Context>,
) -> Result<serde_json::Value, jsonrpsee_core::Error> {
    execute_request(Method::Resume {
        swap_id
    }, context).await
}

async fn get_swap_timelock(
    swap_id: Uuid,
    context: &Arc<Context>,
) -> Result<serde_json::Value, jsonrpsee_core::Error> {
    execute_request(Method::GetSwapExpiredTimelock {
        swap_id
    }, context).await
}

async fn cancel_and_refund_swap(
    swap_id: Uuid,
    context: &Arc<Context>,
) -> Result<serde_json::Value, jsonrpsee_core::Error> {
    execute_request(Method::CancelAndRefund {
        swap_id
    }, context).await
}

async fn withdraw_btc(
    address: bitcoin::Address,
    amount: Option<bitcoin::Amount>,
    context: &Arc<Context>,
) -> Result<serde_json::Value, jsonrpsee_core::Error> {
    execute_request(Method::WithdrawBtc {
        amount,
        address,
    }, context).await
}

async fn buy_xmr(
    bitcoin_change_address: bitcoin::Address,
    monero_receive_address: monero::Address,
    seller: Multiaddr,
    context: &Arc<Context>,
) -> Result<serde_json::Value, jsonrpsee_core::Error> {
    execute_request(Method::BuyXmr {
        seller,
        swap_id: Uuid::new_v4(),
        bitcoin_change_address,
        monero_receive_address
    }, context).await
}

async fn list_sellers(
    rendezvous_point: Multiaddr,
    context: &Arc<Context>,
) -> Result<serde_json::Value, jsonrpsee_core::Error> {
    execute_request(Method::ListSellers {
        rendezvous_point
    }, context).await
}
