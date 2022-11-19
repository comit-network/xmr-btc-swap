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


pub fn register_modules(api_init: &Init) -> RpcModule<()> {
    let mut module = RpcModule::new(());
    module
        .register_async_method("get_bitcoin_balance", |_, _| async {
            get_bitcoin_balance().await.map_err(|err| jsonrpsee_core::Error::Custom(err.to_string()))
        })
        .unwrap();
    module

}

async fn get_bitcoin_balance() -> anyhow::Result<(), Error> {
    let request = Request {
        params: Params::default(),
        cmd: Command::Balance,
    };
   // request.call(api_init).await;
    Ok(())

}
