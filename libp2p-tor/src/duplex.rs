use crate::torut_ext::AuthenticatedConnectionExt;
use crate::{fmt_as_tor_compatible_address, torut_ext, Error};
use fmt_as_tor_compatible_address::fmt_as_tor_compatible_address;
use futures::future::BoxFuture;
use futures::prelude::*;
use libp2p::core::multiaddr::{Multiaddr, Protocol};
use libp2p::core::transport::map_err::MapErr;
use libp2p::core::transport::{ListenerEvent, TransportError};
use libp2p::core::Transport;
use libp2p::futures::stream::BoxStream;
use libp2p::futures::{StreamExt, TryStreamExt};
use libp2p::tcp::{GenTcpConfig, TokioTcpConfig};
use std::sync::Arc;
use tokio::sync::Mutex;
use torut::control::{AsyncEvent, AuthenticatedConn};
use torut::onion::TorSecretKeyV3;

type TorutAsyncEventHandler =
    fn(
        AsyncEvent<'_>,
    ) -> Box<dyn Future<Output = Result<(), torut::control::ConnError>> + Unpin + Send>;

#[derive(Clone)]
pub struct TorConfig {
    inner: MapErr<GenTcpConfig<libp2p::tcp::tokio::Tcp>, fn(std::io::Error) -> Error>, /* TODO: Make generic over async-std / tokio */
    tor_client: Arc<Mutex<AuthenticatedConn<tokio::net::TcpStream, TorutAsyncEventHandler>>>,
    key: TorSecretKeyV3,
    socks_port: u16,
}

impl TorConfig {
    pub async fn new(
        mut client: AuthenticatedConn<tokio::net::TcpStream, TorutAsyncEventHandler>,
        // TODO: change to key directly
        key: TorSecretKeyV3,
    ) -> Result<Self, Error> {
        let socks_port = client.get_socks_port().await?;

        Ok(Self {
            inner: TokioTcpConfig::new().map_err(Error::InnerTransprot),
            tor_client: Arc::new(Mutex::new(client)),
            key,
            socks_port,
        })
    }

    pub async fn from_control_port(
        control_port: u16,
        key: TorSecretKeyV3,
    ) -> Result<Self, Error> {
        let client = AuthenticatedConn::new(control_port).await?;

        Self::new(client, key).await
    }
}

impl Transport for TorConfig {
    type Output = libp2p::tcp::tokio::TcpStream;
    type Error = Error;
    #[allow(clippy::type_complexity)]
    type Listener =
        BoxStream<'static, Result<ListenerEvent<Self::ListenerUpgrade, Self::Error>, Self::Error>>;
    type ListenerUpgrade = BoxFuture<'static, Result<Self::Output, Self::Error>>;
    type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let mut protocols = addr.iter();
        let onion = if let Protocol::Onion3(onion) = protocols
            .next()
            .ok_or_else(|| TransportError::MultiaddrNotSupported(addr.clone()))?
        {
            onion
        } else {
            return Err(TransportError::MultiaddrNotSupported(addr));
        };

        let key: TorSecretKeyV3 = self.key;
        let onion_bytes = key.public().get_onion_address().get_raw_bytes();
        let onion_port = onion.port();

        let localhost_tcp_random_port_addr = format!("/ip4/127.0.0.1/tcp/{}", onion_port).as_str()
            .parse()
            .expect("always a valid multiaddr");

        let listener = self.inner.listen_on(localhost_tcp_random_port_addr)?;

        let tor_client = self.tor_client;

        let listener = listener
            .and_then({
                move |event| {
                    let tor_client = tor_client.clone();
                    let key = key.clone();
                    let onion_multiaddress =
                        Multiaddr::empty().with(Protocol::Onion3((onion_bytes, onion_port).into()));

                    async move {
                        Ok(match event {
                            ListenerEvent::NewAddress(address) => {
                                let local_port = address
                                    .iter()
                                    .find_map(|p| match p {
                                        Protocol::Tcp(port) => Some(port),
                                        _ => None,
                                    })
                                    .expect("TODO: Error handling");

                                // TODO: Don't fully understand this part, why would we have two different multiaddresses here? the actual onion address and the multiaddress would make more sense...?
                                tracing::debug!(
                                    "Setting up hidden service at {} to forward to {}",
                                    onion_multiaddress,
                                    address
                                );

                                match tor_client
                                    .clone()
                                    .lock()
                                    .await
                                    // TODO: Potentially simplify this, in our setup the onion port is always equal to the local port. Otherwise we would have the user provide an additional port for the oion service.
                                    .add_ephemeral_service(&key, onion_port, local_port)
                                    .await
                                {
                                    Ok(()) => ListenerEvent::NewAddress(onion_multiaddress.clone()),
                                    Err(e) => ListenerEvent::Error(Error::Torut(e)),
                                }
                            }
                            ListenerEvent::Upgrade {
                                upgrade,
                                local_addr,
                                remote_addr,
                            } => ListenerEvent::Upgrade {
                                upgrade: upgrade.boxed(),
                                local_addr,
                                remote_addr,
                            },
                            // TODO: why was the constructed multiaddr used here?
                            ListenerEvent::AddressExpired(adr) => {
                                // TODO: even if so, why would we ignore it? Far more logical to just use it...
                                // can ignore address because we only ever listened on one and we
                                // know which one that was

                                let onion_address_without_dot_onion = key
                                    .public()
                                    .get_onion_address()
                                    .get_address_without_dot_onion();

                                match tor_client
                                    .lock()
                                    .await
                                    .del_onion(&onion_address_without_dot_onion)
                                    .await
                                {
                                    Ok(()) => ListenerEvent::AddressExpired(adr),
                                    Err(e) => ListenerEvent::Error(Error::Torut(
                                        torut_ext::Error::Connection(e),
                                    )),
                                }
                            }
                            ListenerEvent::Error(e) => ListenerEvent::Error(e),
                        })
                    }
                }
            })
            .boxed();

        Ok(listener)
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
