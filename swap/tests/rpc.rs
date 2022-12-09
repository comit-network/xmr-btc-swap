use testcontainers::clients::Cli;
use testcontainers::{Container, Docker, RunArgs};
use anyhow::{bail, Context as AnyContext, Result};
use futures::Future;
use swap::api::{Context, Config};
use swap::api::request::{Request, Params, Method, Shutdown};
use std::sync::Arc;
use tokio::time::{interval, timeout};
use std::time::Duration;
pub use jsonrpsee_http_client as http_client;
use tokio::sync::broadcast;

#[cfg(test)]

pub async fn initialize_context() -> (Arc<Context>, Request) {
    let (is_testnet, debug, json) = (true, false, false);
    //let data_dir = data::data_dir_from(None, is_testnet).unwrap();
    let server_address = None;
    let (tx, _) = broadcast::channel(1);

    let mut request = Request {
        params: Params::default(),
        cmd: Method::StartDaemon,
        shutdown: Shutdown::new(tx.subscribe()),
    };

    let context = Context::build(
        None,
        None,
        None,
        None,
        is_testnet,
        debug,
        json,
        server_address,
        tx,
    ).await.unwrap();

    (Arc::new(context), request)
}


#[tokio::test]
pub async fn start_server() {
    let (ctx, mut request) = initialize_context().await;
    let move_ctx = Arc::clone(&ctx);
    tokio::spawn(async move {
        request.call(Arc::clone(&move_ctx)).await;
    });
    tokio::time::sleep(Duration::from_secs(3)).await;
    ctx.shutdown.send(());
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(true);
}


