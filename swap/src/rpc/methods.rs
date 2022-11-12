use jsonrpsee::http_server::{RpcModule};
use crate::api::{InternalApi, Params};
use crate::env::{Config, GetConfig, Testnet};
use crate::fs::system_data_dir;
use url::Url;
use crate::cli::command::{Command, Options};
use std::str::FromStr;
use crate::cli::command::{DEFAULT_ELECTRUM_RPC_URL_TESTNET, DEFAULT_BITCOIN_CONFIRMATION_TARGET_TESTNET};
use crate::rpc::Error;


pub fn register_modules() -> RpcModule<()> {
    let mut module = RpcModule::new(());
    module
        .register_async_method("get_bitcoin_balance", |_, _| async {
            get_bitcoin_balance().await.map_err(|err| jsonrpsee_core::Error::Custom(err.to_string()))
        })
        .unwrap();
    module

}

async fn get_bitcoin_balance() -> anyhow::Result<(), Error> {
    let api = InternalApi {
        opts: Options {
            env_config: Testnet::get_config(),
            debug: false,
            json: true,
            data_dir: system_data_dir().unwrap().join("cli")

        },
        params: Params {
            bitcoin_electrum_rpc_url: Some(Url::from_str(DEFAULT_ELECTRUM_RPC_URL_TESTNET).unwrap()),
            bitcoin_target_block: Some(DEFAULT_BITCOIN_CONFIRMATION_TARGET_TESTNET),
            ..Default::default()
        },
        cmd: Command::Balance,
    };
    api.call().await;
    Ok(())

}
