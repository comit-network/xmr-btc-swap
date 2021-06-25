use std::borrow::Cow;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::num::ParseIntError;
use std::{io, iter};
use torut::control::{AsyncEvent, AuthenticatedConn, TorAuthData, UnauthenticatedConn};
use torut::onion::TorSecretKeyV3;

pub type AsyncEventHandler =
    fn(
        AsyncEvent<'_>,
    ) -> Box<dyn Future<Output = Result<(), torut::control::ConnError>> + Unpin + Send>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Failed to connect to Tor control port")]
    FailedToConnect(#[source] io::Error),
    #[error("Failed to read Tor auth-cookie filed")]
    FailedToReadCookieFile(#[source] io::Error),
    #[error("No authentication information could be found")]
    NoAuthData,
    #[error("Failed to communicate with Tor control port")]
    Connection(torut::control::ConnError),
    #[error("Failed to add hidden service")]
    FailedToAddHiddenService(torut::control::ConnError),
    #[error("Failed to parse port")]
    FailedToParsePort(#[from] ParseIntError),
}

// TODO: Use #[from] once available: https://github.com/teawithsand/torut/issues/12
impl From<torut::control::ConnError> for Error {
    fn from(e: torut::control::ConnError) -> Self {
        Error::Connection(e)
    }
}

#[async_trait::async_trait]
pub trait AuthenticatedConnectionExt: Sized {
    async fn new(control_port: u16) -> Result<Self, Error>;
    async fn with_password(control_port: u16, password: &str) -> Result<Self, Error>;
    async fn add_ephemeral_service(
        &mut self,
        key: &TorSecretKeyV3,
        onion_port: u16,
        local_port: u16,
    ) -> Result<(), Error>;
    async fn get_socks_port(&mut self) -> Result<u16, Error>;
}

#[async_trait::async_trait]
impl AuthenticatedConnectionExt for AuthenticatedConn<tokio::net::TcpStream, AsyncEventHandler> {
    async fn new(control_port: u16) -> Result<Self, Error> {
        let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", control_port))
            .await
            .map_err(Error::FailedToConnect)?;
        let mut uac = UnauthenticatedConn::new(stream);

        let tor_info = uac.load_protocol_info().await?;

        let tor_auth_data = tor_info
            .make_auth_data()
            .map_err(Error::FailedToReadCookieFile)?
            .ok_or(Error::NoAuthData)?;

        uac.authenticate(&tor_auth_data).await?;

        Ok(uac.into_authenticated().await)
    }

    async fn with_password(control_port: u16, password: &str) -> Result<Self, Error> {
        let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", control_port))
            .await
            .map_err(Error::FailedToConnect)?;
        let mut uac = UnauthenticatedConn::new(stream);

        uac.authenticate(&TorAuthData::HashedPassword(Cow::Borrowed(password)))
            .await?;

        Ok(uac.into_authenticated().await)
    }

    async fn add_ephemeral_service(
        &mut self,
        key: &TorSecretKeyV3,
        onion_port: u16,
        local_port: u16,
    ) -> Result<(), Error> {
        tracing::debug!(
            "Adding ephemeral service, onion port {}, local port {}",
            onion_port,
            local_port
        );

        self.add_onion_v3(
            &key,
            false,
            false,
            false,
            None,
            &mut iter::once(&(
                onion_port,
                SocketAddr::new(IpAddr::from(Ipv4Addr::new(127, 0, 0, 1)), local_port),
            )),
        )
        .await
        .map_err(Error::FailedToAddHiddenService)
    }

    async fn get_socks_port(&mut self) -> Result<u16, Error> {
        const DEFAULT_SOCKS_PORT: u16 = 9050;

        let mut vec = self.get_conf("SocksPort").await?;

        let first_element = vec
            .pop()
            .expect("exactly one element because we requested one config option");
        let port = first_element.map_or(Ok(DEFAULT_SOCKS_PORT), |port| port.parse())?; // if config is empty, we are listing on the default port

        Ok(port)
    }
}
