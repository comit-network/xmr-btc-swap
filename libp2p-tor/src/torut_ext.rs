use std::borrow::Cow;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::num::ParseIntError;
use std::{io, iter};
use torut::control::{AsyncEvent, AuthenticatedConn, TorAuthData, UnauthenticatedConn};
use torut::onion::TorSecretKeyV3;

pub type AsyncEventHandler =
    fn(
        AsyncEvent<'_>,
    ) -> Box<dyn Future<Output = Result<(), torut::control::ConnError>> + Unpin + Send>;

#[derive(Debug)]
pub enum Error {
    FailedToConnect(io::Error),
    NoAuthData(Option<io::Error>),
    Connection(torut::control::ConnError),
    FailedToAddHiddenService(torut::control::ConnError),
    FailedToParsePort(ParseIntError),
}

impl From<torut::control::ConnError> for Error {
    fn from(e: torut::control::ConnError) -> Self {
        Error::Connection(e)
    }
}

impl From<ParseIntError> for Error {
    fn from(e: ParseIntError) -> Self {
        Error::FailedToParsePort(e)
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
            .map_err(|e| Error::NoAuthData(Some(e)))?
            .ok_or(Error::NoAuthData(None))?;

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
        println!("Adding ephemeral service, onion port {}, local port {}", onion_port, local_port);

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

        let mut vec = self
            .get_conf("SocksPort")
            .await
            .map_err(Error::Connection)?;

        let first_element = vec
            .pop()
            .expect("exactly one element because we requested one config option");
        let port = first_element.map_or(Ok(DEFAULT_SOCKS_PORT), |port| port.parse())?; // if config is empty, we are listing on the default port

        Ok(port)
    }
}
