use crate::api::Context;
use jsonrpsee::server::{RpcModule, ServerBuilder, ServerHandle};
use std::net::SocketAddr;
use std::sync::Arc;
use thiserror::Error;

pub mod methods;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Could not parse key value from params")]
    ParseError,
}

pub async fn run_server(
    server_address: SocketAddr,
    context: Arc<Context>,
) -> anyhow::Result<(SocketAddr, ServerHandle)> {
    let server = ServerBuilder::default().build(server_address).await?;
    let mut modules = RpcModule::new(());
    {
        modules
            .merge(methods::register_modules(Arc::clone(&context))?)
            .expect("Could not register RPC modules")
    }

    let addr = server.local_addr()?;
    let server_handle = server.start(modules)?;

    Ok((addr, server_handle))
}
