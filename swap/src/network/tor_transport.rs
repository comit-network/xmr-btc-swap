use anyhow::Result;
use data_encoding::BASE32;
use futures::future::{BoxFuture, FutureExt, Ready};
use libp2p::core::multiaddr::{Multiaddr, Protocol};
use libp2p::core::transport::TransportError;
use libp2p::core::Transport;
use libp2p::tcp::tokio::{Tcp, TcpStream};
use libp2p::tcp::TcpListenStream;
use std::io;
use std::net::Ipv4Addr;
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
        let tor_address_string = fmt_as_address_string(addr.clone())?;

        let dial_future = async move {
            tracing::trace!("Connecting through Tor proxy to address: {}", addr);

            let stream =
                Socks5Stream::connect((Ipv4Addr::LOCALHOST, self.socks_port), tor_address_string)
                    .await
                    .map_err(|e| io::Error::new(io::ErrorKind::ConnectionRefused, e))?;

            tracing::trace!("Connection through Tor established");

            Ok(TcpStream(stream.into_inner()))
        };

        Ok(dial_future.boxed())
    }

    fn address_translation(&self, _: &Multiaddr, _: &Multiaddr) -> Option<Multiaddr> {
        None
    }
}

/// Formats the given [`Multiaddr`] as an "address" string.
///
/// For our purposes, we define an address as {HOST}(.{TLD}):{PORT}. This format
/// is what is compatible with the Tor daemon and allows us to route traffic
/// through Tor.
fn fmt_as_address_string(multi: Multiaddr) -> Result<String, TransportError<io::Error>> {
    let mut protocols = multi.iter();

    let address_string = match protocols.next() {
        // if it is an Onion address, we have all we need and can return
        Some(Protocol::Onion3(addr)) => {
            return Ok(format!(
                "{}.onion:{}",
                BASE32.encode(addr.hash()).to_lowercase(),
                addr.port()
            ))
        }
        // Deal with non-onion addresses
        Some(Protocol::Ip4(addr)) => format!("{}", addr),
        Some(Protocol::Ip6(addr)) => format!("{}", addr),
        Some(Protocol::Dns(addr) | Protocol::Dns4(addr)) => format!("{}", addr),
        _ => return Err(TransportError::MultiaddrNotSupported(multi)),
    };

    let port = match protocols.next() {
        Some(Protocol::Tcp(port) | Protocol::Udp(port)) => port,
        _ => return Err(TransportError::MultiaddrNotSupported(multi)),
    };

    Ok(format!("{}:{}", address_string, port))
}

#[cfg(test)]
pub mod test {
    use super::*;

    #[test]
    fn test_tor_address_string() {
        let address =
            "/onion3/oarchy4tamydxcitaki6bc2v4leza6v35iezmu2chg2bap63sv6f2did:1024/p2p/12D3KooWPD4uHN74SHotLN7VCH7Fm8zZgaNVymYcpeF1fpD2guc9"
            ;
        let address_string = fmt_as_address_string(address.parse().unwrap())
            .expect("To be a multi formatted address.");
        assert_eq!(
            address_string,
            "oarchy4tamydxcitaki6bc2v4leza6v35iezmu2chg2bap63sv6f2did.onion:1024"
        );
    }

    #[test]
    fn tcp_to_address_string_should_be_some() {
        let address = "/ip4/127.0.0.1/tcp/7777";
        let address_string = fmt_as_address_string(address.parse().unwrap())
            .expect("To be a formatted multi address. ");
        assert_eq!(address_string, "127.0.0.1:7777");
    }

    #[test]
    fn ip6_to_address_string_should_be_some() {
        let address = "/ip6/2001:db8:85a3:8d3:1319:8a2e:370:7348/tcp/7777";
        let address_string = fmt_as_address_string(address.parse().unwrap())
            .expect("To be a formatted multi address. ");
        assert_eq!(address_string, "2001:db8:85a3:8d3:1319:8a2e:370:7348:7777");
    }

    #[test]
    fn udp_to_address_string_should_be_some() {
        let address = "/ip4/127.0.0.1/udp/7777";
        let address_string = fmt_as_address_string(address.parse().unwrap())
            .expect("To be a formatted multi address. ");
        assert_eq!(address_string, "127.0.0.1:7777");
    }

    #[test]
    fn ws_to_address_string_should_be_some() {
        let address = "/ip4/127.0.0.1/tcp/7777/ws";
        let address_string = fmt_as_address_string(address.parse().unwrap())
            .expect("To be a formatted multi address. ");
        assert_eq!(address_string, "127.0.0.1:7777");
    }

    #[test]
    fn dns4_to_address_string_should_be_some() {
        let address = "/dns4/randomdomain.com/tcp/7777";
        let address_string = fmt_as_address_string(address.parse().unwrap())
            .expect("To be a formatted multi address. ");
        assert_eq!(address_string, "randomdomain.com:7777");
    }

    #[test]
    fn dns_to_address_string_should_be_some() {
        let address = "/dns/randomdomain.com/tcp/7777";
        let address_string = fmt_as_address_string(address.parse().unwrap())
            .expect("To be a formatted multi address. ");
        assert_eq!(address_string, "randomdomain.com:7777");
    }

    #[test]
    fn dnsaddr_to_address_string_should_be_none() {
        let address = "/dnsaddr/randomdomain.com";
        let address_string = fmt_as_address_string(address.parse().unwrap()).ok();
        assert_eq!(address_string, None);
    }
}
