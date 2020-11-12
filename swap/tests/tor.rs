#[cfg(feature = "tor")]
mod tor_test {

    use anyhow::Result;
    use hyper::service::{make_service_fn, service_fn};
    use reqwest::StatusCode;
    use spectral::prelude::*;
    use std::{convert::Infallible, fs};
    use swap::tor::UnauthenticatedConnection;
    use tempfile::{Builder, NamedTempFile};
    use tokio::sync::oneshot::Receiver;
    use torut::{
        onion::TorSecretKeyV3,
        utils::{run_tor, AutoKillChild},
    };

    async fn hello_world(
        _req: hyper::Request<hyper::Body>,
    ) -> Result<hyper::Response<hyper::Body>, Infallible> {
        Ok(hyper::Response::new("Hello World".into()))
    }

    fn start_test_service(port: u16, rx: Receiver<()>) {
        let make_svc =
            make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(hello_world)) });
        let addr = ([127, 0, 0, 1], port).into();
        let server = hyper::Server::bind(&addr).serve(make_svc);
        let graceful = server.with_graceful_shutdown(async {
            rx.await.ok();
        });
        tokio::spawn(async {
            if let Err(e) = graceful.await {
                eprintln!("server error: {}", e);
            }
        });

        tracing::info!("Test server started at port: {}", port);
    }

    fn run_tmp_tor() -> Result<(AutoKillChild, u16, u16, NamedTempFile)> {
        // we create an empty torrc file to not use the system one
        let temp_torrc = Builder::new().tempfile()?;
        let torrc_file = format!("{}", fs::canonicalize(temp_torrc.path())?.display());
        tracing::info!("Temp torrc file created at: {}", torrc_file);

        let control_port = if port_check::is_local_port_free(9051) {
            9051
        } else {
            port_check::free_local_port().unwrap()
        };
        let proxy_port = if port_check::is_local_port_free(9050) {
            9050
        } else {
            port_check::free_local_port().unwrap()
        };

        let child = run_tor(
            "tor",
            &mut [
                "--CookieAuthentication",
                "1",
                "--ControlPort",
                control_port.to_string().as_str(),
                "--SocksPort",
                proxy_port.to_string().as_str(),
                "-f",
                &torrc_file,
            ]
            .iter(),
        )?;
        tracing::info!("Tor running with pid: {}", child.id());
        let child = AutoKillChild::new(child);
        Ok((child, control_port, proxy_port, temp_torrc))
    }

    #[tokio::test]
    async fn test_tor_control_port() -> Result<()> {
        // start tmp tor
        let (_child, control_port, proxy_port, _tmp_torrc) = run_tmp_tor()?;

        // Setup test HTTP Server
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let port = 8080;
        start_test_service(port, rx);

        // Connect to local Tor service
        let mut authenticated_connection =
            UnauthenticatedConnection::with_ports(proxy_port, control_port)
                .init_authenticated_connection()
                .await?;

        tracing::info!("Tor authenticated.");

        // Expose an onion service that re-directs to the echo server.
        let tor_secret_key_v3 = TorSecretKeyV3::generate();
        authenticated_connection
            .add_service(port, &tor_secret_key_v3)
            .await?;

        // Test if Tor service forwards to HTTP Server

        let proxy = reqwest::Proxy::all(format!("socks5h://127.0.0.1:{}", proxy_port).as_str())
            .expect("tor proxy should be there");
        let client = reqwest::Client::builder().proxy(proxy).build()?;
        let onion_address = tor_secret_key_v3.public().get_onion_address().to_string();
        let onion_url = format!("http://{}:8080", onion_address);

        tracing::info!("Tor service added: {}", onion_url);

        let res = client.get(&onion_url).send().await?;

        assert_that(&res.status()).is_equal_to(StatusCode::OK);

        let text = res.text().await?;
        assert_that!(text).contains("Hello World");
        tracing::info!(
            "Local server called via Tor proxy. Its response is: {}",
            text
        );

        // gracefully shut down server
        let _ = tx.send(());
        Ok(())
    }
}
