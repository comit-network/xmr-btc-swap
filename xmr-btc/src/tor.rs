use anyhow::{anyhow, bail, Result};
use lazy_static::lazy_static;
use std::{
    future::Future,
    net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4},
};
use tokio::net::TcpStream;
use torut::{
    control::{AsyncEvent, AuthenticatedConn, ConnError, UnauthenticatedConn},
    onion::TorSecretKeyV3,
};

lazy_static! {
    /// The default TOR socks5 proxy address, `127.0.0.1:9050`.
    pub static ref TOR_PROXY_ADDR: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 9050);
    /// The default TOR Controller Protocol address, `127.0.0.1:9051`.
    pub static ref TOR_CP_ADDR: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 9051));
}

/// checks if tor is running
async fn tor_running() -> Result<()> {
    // Make sure you are running tor and this is your socks port
    let proxy = reqwest::Proxy::all(format!("socks5h://{}", *TOR_PROXY_ADDR).as_str())
        .expect("tor proxy should be there");
    let client = reqwest::Client::builder()
        .proxy(proxy)
        .build()
        .expect("should be able to build reqwest client");

    let res = client.get("https://check.torproject.org").send().await?;

    let text = res.text().await?;
    let is_tor = text.contains("Congratulations. This browser is configured to use Tor.");

    if is_tor {
        Ok(())
    } else {
        bail!("Tor is currently not running")
    }
}

type Handler = fn(AsyncEvent<'_>) -> Box<dyn Future<Output = Result<(), ConnError>> + Unpin>;

#[allow(missing_debug_implementations)]
pub struct AuthenticatedConnection(AuthenticatedConn<TcpStream, Handler>);

impl AuthenticatedConnection {
    async fn init_unauthenticated_connection() -> Result<UnauthenticatedConn<TcpStream>> {
        // try to connect to local tor service via control port
        let sock = TcpStream::connect(*TOR_CP_ADDR).await?;
        let unauthenticated_connection = UnauthenticatedConn::new(sock);
        Ok(unauthenticated_connection)
    }

    /// Create a new authenticated connection to your local Tor service
    pub async fn new() -> Result<Self> {
        tor_running().await?;

        let mut unauthenticated_connection = match Self::init_unauthenticated_connection().await {
            Err(_) => bail!("Tor instance not running"),
            Ok(unauthenticated_connection) => unauthenticated_connection,
        };

        let tor_info = match unauthenticated_connection.load_protocol_info().await {
            Ok(info) => info,
            Err(_) => bail!("Failed to load protocol info from Tor."),
        };
        let tor_auth_data = tor_info
            .make_auth_data()?
            .expect("Failed to make auth data.");

        // Get an authenticated connection to the Tor via the Tor Controller protocol.
        if unauthenticated_connection
            .authenticate(&tor_auth_data)
            .await
            .is_err()
        {
            bail!("Failed to authenticate with Tor")
        }
        let authenticated_connection = unauthenticated_connection.into_authenticated().await;

        Ok(AuthenticatedConnection(authenticated_connection))
    }

    /// Add an ephemeral tor service on localhost with the provided key
    pub async fn add_service(&mut self, port: u16, tor_key: &TorSecretKeyV3) -> Result<()> {
        self.0
            .add_onion_v3(
                tor_key,
                false,
                false,
                false,
                None,
                &mut [(
                    port,
                    SocketAddr::new(IpAddr::from(Ipv4Addr::new(127, 0, 0, 1)), port),
                )]
                .iter(),
            )
            .await
            .map_err(|_| anyhow!("Could not add onion service."))?;
        Ok(())
    }
}
