use crate::torut_ext::AuthenticatedConnectionExt;
use crate::{fmt_as_tor_compatible_address, Error};
use anyhow::Result;
use fmt_as_tor_compatible_address::fmt_as_tor_compatible_address;
use futures::future::BoxFuture;
use futures::prelude::*;
use libp2p::core::multiaddr::Multiaddr;
use libp2p::core::transport::{ListenerEvent, TransportError};
use libp2p::core::Transport;
use libp2p::futures::stream::BoxStream;
use libp2p::tcp::tokio::TcpStream;
use torut::control::AuthenticatedConn;

#[derive(Clone)]
pub struct TorConfig {
    socks_port: u16,
}

impl TorConfig {
    pub fn new(socks_port: u16) -> Self {
        Self { socks_port }
    }

    pub async fn from_control_port(control_port: u16) -> Result<Self, Error> {
        let mut client = AuthenticatedConn::new(control_port).await?;
        let socks_port = client.get_socks_port().await?;

        Ok(Self::new(socks_port))
    }
}

impl Transport for TorConfig {
    type Output = TcpStream;
    type Error = Error;
    #[allow(clippy::type_complexity)]
    type Listener =
        BoxStream<'static, Result<ListenerEvent<Self::ListenerUpgrade, Self::Error>, Self::Error>>;
    type ListenerUpgrade = BoxFuture<'static, Result<Self::Output, Self::Error>>;
    type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        tracing::debug!("Connecting through Tor proxy to address {}", addr);

        let address = fmt_as_tor_compatible_address(addr.clone())
            .ok_or(TransportError::MultiaddrNotSupported(addr))?;

        Ok(crate::dial_via_tor(address, self.socks_port).boxed())
    }

    fn address_translation(&self, _: &Multiaddr, _: &Multiaddr) -> Option<Multiaddr> {
        None // address translation for tor doesn't make any sense :)
    }
}
