use crate::cli::api::Context;
use std::net::SocketAddr;
use thiserror::Error;
use tower_http::cors::CorsLayer;

use jsonrpsee::{
    core::server::host_filtering::AllowHosts,
    server::{ServerBuilder, ServerHandle},
};

pub mod methods;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Could not parse key value from params")]
    ParseError,
}

pub async fn run_server(
    server_address: SocketAddr,
    context: Context,
) -> anyhow::Result<(SocketAddr, ServerHandle)> {
    let cors = CorsLayer::permissive();
    let middleware = tower::ServiceBuilder::new().layer(cors);

    let server = ServerBuilder::default()
        .set_host_filtering(AllowHosts::Any)
        .set_middleware(middleware)
        .build(server_address)
        .await?;

    let modules = methods::register_modules(context)?;

    let addr = server.local_addr()?;
    let server_handle = server.start(modules)?;

    Ok((addr, server_handle))
}
