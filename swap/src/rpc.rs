use std::net::SocketAddr;
use jsonrpsee::http_server::{RpcModule, HttpServerBuilder, HttpServerHandle};
use thiserror::Error;
use crate::api::{Init};
use std::sync::Arc;

pub mod methods;

#[derive(Debug, Error)]
pub enum Error {
    #[error("example")]
    ExampleError,
}

pub async fn run_server(server_address: SocketAddr, api_init: Arc<Init>) -> anyhow::Result<(SocketAddr, HttpServerHandle)> {
	let server = HttpServerBuilder::default().build(server_address).await?;
    let mut modules = RpcModule::new(());
    {
        modules.merge(methods::register_modules(Arc::clone(&api_init)))
            .unwrap()
    }

	let addr = server.local_addr()?;
	let server_handle = server.start(modules)?;
	Ok((addr, server_handle))
}

