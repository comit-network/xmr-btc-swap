use anyhow::{bail, Context, Result};
use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use tokio::net::TcpStream;
use torut::control::{AsyncEvent, AuthenticatedConn, ConnError, UnauthenticatedConn};
use torut::onion::TorSecretKeyV3;

pub const DEFAULT_SOCKS5_PORT: u16 = 9050;
pub const DEFAULT_CONTROL_PORT: u16 = 9051;

#[derive(Debug, Clone, Copy)]
pub struct Client {
    socks5_address: SocketAddrV4,
    control_port_address: SocketAddr,
}

impl Default for Client {
    fn default() -> Self {
        Self {
            socks5_address: SocketAddrV4::new(Ipv4Addr::LOCALHOST, DEFAULT_SOCKS5_PORT),
            control_port_address: SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::LOCALHOST,
                DEFAULT_CONTROL_PORT,
            )),
        }
    }
}

impl Client {
    pub fn new(socks5_port: u16) -> Self {
        Self {
            socks5_address: SocketAddrV4::new(Ipv4Addr::LOCALHOST, socks5_port),
            control_port_address: SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::LOCALHOST,
                DEFAULT_CONTROL_PORT,
            )),
        }
    }
    pub fn with_control_port(self, control_port: u16) -> Self {
        Self {
            control_port_address: SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::LOCALHOST,
                control_port,
            )),
            ..self
        }
    }

    /// checks if tor is running
    pub async fn assert_tor_running(&self) -> Result<()> {
        // Make sure you are running tor and this is your socks port
        let proxy = reqwest::Proxy::all(format!("socks5h://{}", self.socks5_address).as_str())
            .context("Failed to construct Tor proxy URL")?;
        let client = reqwest::Client::builder().proxy(proxy).build()?;

        let res = client.get("https://check.torproject.org").send().await?;
        let text = res.text().await?;

        if !text.contains("Congratulations. This browser is configured to use Tor.") {
            bail!("Tor is currently not running")
        }

        Ok(())
    }

    async fn init_unauthenticated_connection(&self) -> Result<UnauthenticatedConn<TcpStream>> {
        // Connect to local tor service via control port
        let sock = TcpStream::connect(self.control_port_address).await?;
        let uc = UnauthenticatedConn::new(sock);
        Ok(uc)
    }

    /// Create a new authenticated connection to your local Tor service
    pub async fn into_authenticated_client(self) -> Result<AuthenticatedClient> {
        self.assert_tor_running().await?;

        let mut uc = self
            .init_unauthenticated_connection()
            .await
            .context("Failed to connect to Tor")?;

        let tor_info = uc
            .load_protocol_info()
            .await
            .context("Failed to load protocol info from Tor")?;

        let tor_auth_data = tor_info
            .make_auth_data()?
            .context("Failed to make Tor auth data")?;

        // Get an authenticated connection to the Tor via the Tor Controller protocol.
        uc.authenticate(&tor_auth_data)
            .await
            .context("Failed to authenticate with Tor")?;

        Ok(AuthenticatedClient {
            inner: uc.into_authenticated().await,
        })
    }

    pub fn tor_proxy_port(&self) -> u16 {
        self.socks5_address.port()
    }
}

type Handler = fn(AsyncEvent<'_>) -> Box<dyn Future<Output = Result<(), ConnError>> + Unpin>;

#[allow(missing_debug_implementations)]
pub struct AuthenticatedClient {
    inner: AuthenticatedConn<TcpStream, Handler>,
}

impl AuthenticatedClient {
    /// Add an ephemeral tor service on localhost with the provided key
    /// `service_port` and `onion_port` can be different but don't have to as
    /// they are on different networks.
    pub async fn add_services(
        &mut self,
        services: &[(u16, SocketAddr)],
        tor_key: &TorSecretKeyV3,
    ) -> Result<()> {
        let mut listeners = services.iter();
        self.inner
            .add_onion_v3(tor_key, false, false, false, None, &mut listeners)
            .await
            .context("Failed to add onion service")
    }
}
