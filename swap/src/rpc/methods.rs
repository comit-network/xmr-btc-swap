use crate::api::request::{
    get_current_swap, get_history, get_raw_states,
    suspend_current_swap,
    BalanceArgs, BuyXmrArgs, CancelAndRefundArgs, GetSwapInfoArgs, ListSellersArgs,
    MoneroRecoveryArgs, Request, ResumeSwapArgs, WithdrawBtcArgs,
};
use crate::api::Context;
use crate::bitcoin::bitcoin_address;
use crate::monero::monero_address;
use crate::{bitcoin, monero};
use anyhow::Result;
use jsonrpsee::server::RpcModule;
use libp2p::core::Multiaddr;
use std::collections::HashMap;
use std::str::FromStr;
use uuid::Uuid;

trait ConvertToJsonRpseeError<T> {
    fn to_jsonrpsee_result(self) -> Result<T, jsonrpsee_core::Error>;
}

impl<T> ConvertToJsonRpseeError<T> for Result<T, anyhow::Error> {
    fn to_jsonrpsee_result(self) -> Result<T, jsonrpsee_core::Error> {
        self.map_err(|e| jsonrpsee_core::Error::Custom(e.to_string()))
    }
}

pub fn register_modules(outer_context: Context) -> Result<RpcModule<Context>> {
    let mut module = RpcModule::new(outer_context);

    module.register_async_method("suspend_current_swap", |_, context| async move {
        suspend_current_swap(context).await.to_jsonrpsee_result()
    })?;

    module.register_async_method("get_swap_info", |params_raw, context| async move {
        let params: HashMap<String, serde_json::Value> = params_raw.parse()?;

        let swap_id = params
            .get("swap_id")
            .ok_or_else(|| jsonrpsee_core::Error::Custom("Does not contain swap_id".to_string()))?;

        let swap_id = as_uuid(swap_id)
            .ok_or_else(|| jsonrpsee_core::Error::Custom("Could not parse swap_id".to_string()))?;

        GetSwapInfoArgs { swap_id }
            .request(context)
            .await
            .to_jsonrpsee_result()
    })?;

    module.register_async_method("get_bitcoin_balance", |params_raw, context| async move {
        let params: HashMap<String, serde_json::Value> = params_raw.parse()?;

        let force_refresh = params
            .get("force_refresh")
            .ok_or_else(|| {
                jsonrpsee_core::Error::Custom("Does not contain force_refresh".to_string())
            })?
            .as_bool()
            .ok_or_else(|| {
                jsonrpsee_core::Error::Custom("force_refesh is not a boolean".to_string())
            })?;

        BalanceArgs { force_refresh }
            .request(context)
            .await
            .to_jsonrpsee_result()
    })?;

    module.register_async_method("get_history", |_, context| async move {
        get_history(context).await.to_jsonrpsee_result()
    })?;

    module.register_async_method("get_raw_states", |_, context| async move {
        get_raw_states(context).await.to_jsonrpsee_result()
    })?;

    module.register_async_method("resume_swap", |params_raw, context| async move {
        let params: HashMap<String, serde_json::Value> = params_raw.parse()?;

        let swap_id = params
            .get("swap_id")
            .ok_or_else(|| jsonrpsee_core::Error::Custom("Does not contain swap_id".to_string()))?;

        let swap_id = as_uuid(swap_id)
            .ok_or_else(|| jsonrpsee_core::Error::Custom("Could not parse swap_id".to_string()))?;

        ResumeSwapArgs { swap_id }
            .request(context)
            .await
            .to_jsonrpsee_result()
    })?;

    module.register_async_method("cancel_refund_swap", |params_raw, context| async move {
        let params: HashMap<String, serde_json::Value> = params_raw.parse()?;

        let swap_id = params
            .get("swap_id")
            .ok_or_else(|| jsonrpsee_core::Error::Custom("Does not contain swap_id".to_string()))?;

        let swap_id = as_uuid(swap_id)
            .ok_or_else(|| jsonrpsee_core::Error::Custom("Could not parse swap_id".to_string()))?;

        CancelAndRefundArgs { swap_id }
            .request(context)
            .await
            .to_jsonrpsee_result()
    })?;

    module.register_async_method(
        "get_monero_recovery_info",
        |params_raw, context| async move {
            let params: HashMap<String, serde_json::Value> = params_raw.parse()?;

            let swap_id = params.get("swap_id").ok_or_else(|| {
                jsonrpsee_core::Error::Custom("Does not contain swap_id".to_string())
            })?;

            let swap_id = as_uuid(swap_id).ok_or_else(|| {
                jsonrpsee_core::Error::Custom("Could not parse swap_id".to_string())
            })?;

            MoneroRecoveryArgs { swap_id }
                .request(context)
                .await
                .to_jsonrpsee_result()
        },
    )?;

    module.register_async_method("withdraw_btc", |params_raw, context| async move {
        let params: HashMap<String, String> = params_raw.parse()?;

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
            bitcoin_address::validate(withdraw_address, context.config.env_config.bitcoin_network)?;

        WithdrawBtcArgs {
            amount,
            address: withdraw_address,
        }
        .request(context)
        .await
        .to_jsonrpsee_result()
    })?;

    module.register_async_method("buy_xmr", |params_raw, context| async move {
        let params: HashMap<String, String> = params_raw.parse()?;

        let bitcoin_change_address =
            bitcoin::Address::from_str(params.get("bitcoin_change_address").ok_or_else(|| {
                jsonrpsee_core::Error::Custom("Does not contain bitcoin_change_address".to_string())
            })?)
            .map_err(|err| jsonrpsee_core::Error::Custom(err.to_string()))?;

        let bitcoin_change_address = bitcoin_address::validate(
            bitcoin_change_address,
            context.config.env_config.bitcoin_network,
        )?;

        let monero_receive_address =
            monero::Address::from_str(params.get("monero_receive_address").ok_or_else(|| {
                jsonrpsee_core::Error::Custom("Does not contain monero_receiveaddress".to_string())
            })?)
            .map_err(|err| jsonrpsee_core::Error::Custom(err.to_string()))?;

        let monero_receive_address = monero_address::validate(
            monero_receive_address,
            context.config.env_config.monero_network,
        )?;

        let seller =
            Multiaddr::from_str(params.get("seller").ok_or_else(|| {
                jsonrpsee_core::Error::Custom("Does not contain seller".to_string())
            })?)
            .map_err(|err| jsonrpsee_core::Error::Custom(err.to_string()))?;

        BuyXmrArgs {
            seller,
            bitcoin_change_address,
            monero_receive_address,
        }
        .request(context)
        .await
        .to_jsonrpsee_result()
    })?;

    module.register_async_method("list_sellers", |params_raw, context| async move {
        let params: HashMap<String, serde_json::Value> = params_raw.parse()?;

        let rendezvous_point = params.get("rendezvous_point").ok_or_else(|| {
            jsonrpsee_core::Error::Custom("Does not contain rendezvous_point".to_string())
        })?;

        let rendezvous_point = rendezvous_point
            .as_str()
            .and_then(|addr_str| Multiaddr::from_str(addr_str).ok())
            .ok_or_else(|| {
                jsonrpsee_core::Error::Custom("Could not parse valid multiaddr".to_string())
            })?;

        ListSellersArgs {
            rendezvous_point: rendezvous_point.clone(),
        }
        .request(context)
        .await
        .to_jsonrpsee_result()
    })?;

    module.register_async_method("get_current_swap", |_, context| async move {
        get_current_swap(context).await.to_jsonrpsee_result()
    })?;

    Ok(module)
}

fn as_uuid(json_value: &serde_json::Value) -> Option<Uuid> {
    if let Some(uuid_str) = json_value.as_str() {
        Uuid::parse_str(uuid_str).ok()
    } else {
        None
    }
}
