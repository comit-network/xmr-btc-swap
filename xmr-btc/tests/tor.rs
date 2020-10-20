use anyhow::Result;
use hyper::service::{make_service_fn, service_fn};
use reqwest::StatusCode;
use spectral::prelude::*;
use std::convert::Infallible;
use tokio::sync::oneshot::Receiver;
use torut::onion::TorSecretKeyV3;
use xmr_btc::tor::{AuthenticatedConnection, TOR_PROXY_ADDR};

async fn hello_world(
    _req: hyper::Request<hyper::Body>,
) -> Result<hyper::Response<hyper::Body>, Infallible> {
    Ok(hyper::Response::new("Hello World".into()))
}

fn start_test_service(port: u16, rx: Receiver<()>) {
    let make_svc = make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(hello_world)) });
    let addr = ([127, 0, 0, 1], port).into();
    let server = hyper::Server::bind(&addr).serve(make_svc);
    let graceful = server.with_graceful_shutdown(async {
        rx.await.ok();
    });
    tokio::spawn(async {
        // server.await.unwrap();
        if let Err(e) = graceful.await {
            eprintln!("server error: {}", e);
        }
    });
}

#[tokio::test]
async fn test_tor_control_port() -> Result<()> {
    // Setup test HTTP Server
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let port = 8080;
    start_test_service(port, rx);

    // Connect to local Tor service
    let mut authenticated_connection = AuthenticatedConnection::new().await?;

    // Expose an onion service that re-directs to the echo server.
    let tor_secret_key_v3 = TorSecretKeyV3::generate();
    authenticated_connection
        .add_service(port, &tor_secret_key_v3)
        .await?;

    // Test if Tor service forwards to HTTP Server

    let proxy = reqwest::Proxy::all(format!("socks5h://{}", *TOR_PROXY_ADDR).as_str())
        .expect("tor proxy should be there");
    let client = reqwest::Client::builder().proxy(proxy).build().unwrap();
    let onion_address = tor_secret_key_v3.public().get_onion_address().to_string();
    let onion_url = format!("http://{}:8080", onion_address);

    let res = client.get(&onion_url).send().await?;
    assert_that(&res.status()).is_equal_to(StatusCode::OK);

    let text = res.text().await?;
    assert_that!(text).contains("Hello World");

    // gracefully shut down server
    let _ = tx.send(());
    Ok(())
}
