use anyhow::Result;
use data_encoding::BASE32;
use futures::future::{BoxFuture, FutureExt, Ready};
use libp2p::core::multiaddr::{Multiaddr, Protocol};
use libp2p::core::transport::TransportError;
use libp2p::core::Transport;
use libp2p::tcp::tokio::{Tcp, TcpStream};
use libp2p::tcp::TcpListenStream;
use std::borrow::Cow;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::{fmt, io};
use tokio_socks::tcp::Socks5Stream;

/// A [`Transport`] that can dial onion addresses through a running Tor daemon.
#[derive(Clone)]
pub struct TorDialOnlyTransport {
    socks_port: u16,
}

impl TorDialOnlyTransport {
    pub fn new(socks_port: u16) -> Self {
        Self { socks_port }
    }
}

impl Transport for TorDialOnlyTransport {
    type Output = TcpStream;
    type Error = io::Error;
    type Listener = TcpListenStream<Tcp>;
    type ListenerUpgrade = Ready<Result<Self::Output, Self::Error>>;
    type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let address = TorCompatibleAddress::from_multiaddr(Cow::Borrowed(&addr))?;

        if address.is_certainly_not_reachable_via_tor_daemon() {
            return Err(TransportError::MultiaddrNotSupported(addr));
        }

        let dial_future = async move {
            tracing::debug!(address = %addr, "Establishing connection through Tor proxy");

            let stream =
                Socks5Stream::connect((Ipv4Addr::LOCALHOST, self.socks_port), address.to_string())
                    .await
                    .map_err(|e| io::Error::new(io::ErrorKind::ConnectionRefused, e))?;

            tracing::debug!("Connection through Tor established");

            Ok(TcpStream(stream.into_inner()))
        };

        Ok(dial_future.boxed())
    }

    fn address_translation(&self, _: &Multiaddr, _: &Multiaddr) -> Option<Multiaddr> {
        None
    }
    fn dial_as_listener(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let address = TorCompatibleAddress::from_multiaddr(Cow::Borrowed(&addr))?;

        if address.is_certainly_not_reachable_via_tor_daemon() {
            return Err(TransportError::MultiaddrNotSupported(addr));
        }

        let dial_future = async move {
            tracing::debug!(address = %addr, "Establishing connection through Tor proxy");

            let stream =
                Socks5Stream::connect((Ipv4Addr::LOCALHOST, self.socks_port), address.to_string())
                    .await
                    .map_err(|e| io::Error::new(io::ErrorKind::ConnectionRefused, e))?;

            tracing::debug!("Connection through Tor established");

            Ok(TcpStream(stream.into_inner()))
        };

        Ok(dial_future.boxed())
    }
}

/// Represents an address that is _compatible_ with Tor, i.e. can be resolved by
/// the Tor daemon.
#[derive(Debug)]
enum TorCompatibleAddress {
    Onion3 { host: String, port: u16 },
    Dns { address: String, port: u16 },
    Ip4 { address: Ipv4Addr, port: u16 },
    Ip6 { address: Ipv6Addr, port: u16 },
}

impl TorCompatibleAddress {
    /// Constructs a new [`TorCompatibleAddress`] from a [`Multiaddr`].
    fn from_multiaddr(multi: Cow<'_, Multiaddr>) -> Result<Self, TransportError<io::Error>> {
        match multi.iter().collect::<Vec<_>>().as_slice() {
            [Protocol::Onion3(onion), ..] => Ok(TorCompatibleAddress::Onion3 {
                host: BASE32.encode(onion.hash()).to_lowercase(),
                port: onion.port(),
            }),
            [Protocol::Ip4(address), Protocol::Tcp(port) | Protocol::Udp(port), ..] => {
                Ok(TorCompatibleAddress::Ip4 {
                    address: *address,
                    port: *port,
                })
            }
            [Protocol::Dns(address) | Protocol::Dns4(address), Protocol::Tcp(port) | Protocol::Udp(port), ..] => {
                Ok(TorCompatibleAddress::Dns {
                    address: format!("{}", address),
                    port: *port,
                })
            }
            [Protocol::Ip6(address), Protocol::Tcp(port) | Protocol::Udp(port), ..] => {
                Ok(TorCompatibleAddress::Ip6 {
                    address: *address,
                    port: *port,
                })
            }
            _ => Err(TransportError::MultiaddrNotSupported(multi.into_owned())),
        }
    }

    /// Checks if the address is reachable via the Tor daemon.
    ///
    /// The Tor daemon can dial onion addresses, resolve DNS names and dial
    /// IP4/IP6 addresses reachable via the public Internet.
    /// We can't guarantee that an address is reachable via the Internet but we
    /// can say that some addresses are almost certainly not reachable, for
    /// example, loopback addresses.
    fn is_certainly_not_reachable_via_tor_daemon(&self) -> bool {
        match self {
            TorCompatibleAddress::Onion3 { .. } => false,
            TorCompatibleAddress::Dns { address, .. } => address == "localhost",
            TorCompatibleAddress::Ip4 { address, .. } => address.is_loopback(),
            TorCompatibleAddress::Ip6 { address, .. } => address.is_loopback(),
        }
    }
}

impl fmt::Display for TorCompatibleAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TorCompatibleAddress::Onion3 { host, port } => write!(f, "{}.onion:{}", host, port),
            TorCompatibleAddress::Dns { address, port } => write!(f, "{}:{}", address, port),
            TorCompatibleAddress::Ip4 { address, port } => write!(f, "{}:{}", address, port),
            TorCompatibleAddress::Ip6 { address, port } => write!(f, "{}:{}", address, port),
        }
    }
}

#[cfg(test)]
pub mod test {
    use super::*;

    #[test]
    fn test_tor_address_string() {
        let address = tor_compatible_address_from_str("/onion3/oarchy4tamydxcitaki6bc2v4leza6v35iezmu2chg2bap63sv6f2did:1024/p2p/12D3KooWPD4uHN74SHotLN7VCH7Fm8zZgaNVymYcpeF1fpD2guc9");

        assert!(!address.is_certainly_not_reachable_via_tor_daemon());
        assert_eq!(
            address.to_string(),
            "oarchy4tamydxcitaki6bc2v4leza6v35iezmu2chg2bap63sv6f2did.onion:1024"
        );
    }

    #[test]
    fn tcp_to_address_string_should_be_some() {
        let address = tor_compatible_address_from_str("/ip4/127.0.0.1/tcp/7777");

        assert!(address.is_certainly_not_reachable_via_tor_daemon());
        assert_eq!(address.to_string(), "127.0.0.1:7777");
    }

    #[test]
    fn ip6_to_address_string_should_be_some() {
        let address =
            tor_compatible_address_from_str("/ip6/2001:db8:85a3:8d3:1319:8a2e:370:7348/tcp/7777");

        assert!(!address.is_certainly_not_reachable_via_tor_daemon());
        assert_eq!(
            address.to_string(),
            "2001:db8:85a3:8d3:1319:8a2e:370:7348:7777"
        );
    }

    #[test]
    fn udp_to_address_string_should_be_some() {
        let address = tor_compatible_address_from_str("/ip4/127.0.0.1/udp/7777");

        assert!(address.is_certainly_not_reachable_via_tor_daemon());
        assert_eq!(address.to_string(), "127.0.0.1:7777");
    }

    #[test]
    fn ws_to_address_string_should_be_some() {
        let address = tor_compatible_address_from_str("/ip4/127.0.0.1/tcp/7777/ws");

        assert!(address.is_certainly_not_reachable_via_tor_daemon());
        assert_eq!(address.to_string(), "127.0.0.1:7777");
    }

    #[test]
    fn dns4_to_address_string_should_be_some() {
        let address = tor_compatible_address_from_str("/dns4/randomdomain.com/tcp/7777");

        assert!(!address.is_certainly_not_reachable_via_tor_daemon());
        assert_eq!(address.to_string(), "randomdomain.com:7777");
    }

    #[test]
    fn dns_to_address_string_should_be_some() {
        let address = tor_compatible_address_from_str("/dns/randomdomain.com/tcp/7777");

        assert!(!address.is_certainly_not_reachable_via_tor_daemon());
        assert_eq!(address.to_string(), "randomdomain.com:7777");
    }

    #[test]
    fn dnsaddr_to_address_string_should_be_error() {
        let address = "/dnsaddr/randomdomain.com";

        let _ =
            TorCompatibleAddress::from_multiaddr(Cow::Owned(address.parse().unwrap())).unwrap_err();
    }

    fn tor_compatible_address_from_str(str: &str) -> TorCompatibleAddress {
        TorCompatibleAddress::from_multiaddr(Cow::Owned(str.parse().unwrap())).unwrap()
    }
}
