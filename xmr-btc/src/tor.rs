use anyhow::{anyhow, bail, Result};
use std::{
    future::Future,
    net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4},
};
use tokio::net::TcpStream;
use torut::{
    control::{AsyncEvent, AuthenticatedConn, ConnError, UnauthenticatedConn},
    onion::TorSecretKeyV3,
};

#[derive(Debug, Clone, Copy)]
pub struct UnauthenticatedConnection {
    tor_proxy_address: SocketAddrV4,
    tor_control_port_address: SocketAddr,
}

impl Default for UnauthenticatedConnection {
    fn default() -> Self {
        Self {
            tor_proxy_address: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 9050),
            tor_control_port_address: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 9051)),
        }
    }
}

impl UnauthenticatedConnection {
    pub fn with_ports(proxy_port: u16, control_port: u16) -> Self {
        Self {
            tor_proxy_address: SocketAddrV4::new(Ipv4Addr::LOCALHOST, proxy_port),
            tor_control_port_address: SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::LOCALHOST,
                control_port,
            )),
        }
    }

    /// checks if tor is running
    async fn tor_running(&self) -> Result<()> {
        // Make sure you are running tor and this is your socks port
        let proxy = reqwest::Proxy::all(format!("socks5h://{}", self.tor_proxy_address).as_str())
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

    async fn init_unauthenticated_connection(&self) -> Result<UnauthenticatedConn<TcpStream>> {
        // Connect to local tor service via control port
        let sock = TcpStream::connect(self.tor_control_port_address).await?;
        let unauthenticated_connection = UnauthenticatedConn::new(sock);
        Ok(unauthenticated_connection)
    }

    /// Create a new authenticated connection to your local Tor service
    pub async fn init_authenticated_connection(self) -> Result<AuthenticatedConnection> {
        self.tor_running().await?;

        let mut unauthenticated_connection = match self.init_unauthenticated_connection().await {
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

        Ok(AuthenticatedConnection {
            authenticated_connection,
        })
    }
}

type Handler = fn(AsyncEvent<'_>) -> Box<dyn Future<Output = Result<(), ConnError>> + Unpin>;

#[allow(missing_debug_implementations)]
pub struct AuthenticatedConnection {
    authenticated_connection: AuthenticatedConn<TcpStream, Handler>,
}

impl AuthenticatedConnection {
    /// Add an ephemeral tor service on localhost with the provided key
    pub async fn add_service(&mut self, port: u16, tor_key: &TorSecretKeyV3) -> Result<()> {
        self.authenticated_connection
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
