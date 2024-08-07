use crate::api::Context;
use std::{net::SocketAddr, sync::Arc};
use thiserror::Error;
use tower_http::cors::CorsLayer;

use jsonrpsee::{
    core::server::host_filtering::AllowHosts,
    server::{RpcModule, ServerBuilder, ServerHandle},
};

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
    let cors = CorsLayer::permissive();
    let middleware = tower::ServiceBuilder::new().layer(cors);

    let server = ServerBuilder::default()
        .set_host_filtering(AllowHosts::Any)
        .set_middleware(middleware)
        .build(server_address)
        .await?;
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
