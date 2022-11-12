use std::net::SocketAddr;
use jsonrpsee::http_server::{RpcModule, HttpServerBuilder, HttpServerHandle};

pub async fn run_server(server_address: SocketAddr) -> anyhow::Result<(SocketAddr, HttpServerHandle)> {
	let server = HttpServerBuilder::default().build(server_address).await?;
	let mut module = RpcModule::new(());
	module.register_async_method("balance", |_, _| get_balance())?;

	let addr = server.local_addr()?;
	let server_handle = server.start(module)?;
	Ok((addr, server_handle))
}

async fn get_balance() -> Result<&'static str, jsonrpsee::core::Error> {
    Ok("hey")
}
