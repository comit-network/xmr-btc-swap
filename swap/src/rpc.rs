use crate::api::Context;
use jsonrpsee::server::{ServerBuilder, ServerHandle, RpcModule};
use std::net::SocketAddr;
use std::sync::Arc;
use thiserror::Error;

pub mod methods;

#[derive(Debug, Error)]
pub enum Error {
    #[error("example")]
    ExampleError,
}

pub async fn run_server(
    server_address: SocketAddr,
    context: Arc<Context>,
) -> anyhow::Result<(SocketAddr, ServerHandle)> {
    let server = ServerBuilder::default().build(server_address).await?;
    let mut modules = RpcModule::new(());
    {
        modules
            .merge(methods::register_modules(Arc::clone(&context)))
            .unwrap()
    }

    let addr = server.local_addr()?;
    let server_handle = server.start(modules)?;
    tracing::info!(%addr, "Started RPC server");

    Ok((addr, server_handle))
}
