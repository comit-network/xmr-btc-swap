use anyhow::{anyhow, Result};
use data_encoding::BASE32;
use futures::future::{BoxFuture, FutureExt, Ready};
use libp2p::core::multiaddr::{Multiaddr, Protocol};
use libp2p::core::transport::TransportError;
use libp2p::core::Transport;
use libp2p::tcp::tokio::{Tcp, TcpStream};
use libp2p::tcp::{GenTcpConfig, TcpListenStream, TokioTcpConfig};
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use tokio_socks::tcp::Socks5Stream;

/// Represents the configuration for a Tor transport for libp2p.
#[derive(Clone)]
pub struct TorTcpConfig {
    inner: GenTcpConfig<Tcp>,
    /// Tor SOCKS5 proxy port number.
    socks_port: u16,
}

impl TorTcpConfig {
    pub fn new(tcp: TokioTcpConfig, socks_port: u16) -> Self {
        Self {
            inner: tcp,
            socks_port,
        }
    }
}

impl Transport for TorTcpConfig {
    type Output = TcpStream;
    type Error = io::Error;
    type Listener = TcpListenStream<Tcp>;
    type ListenerUpgrade = Ready<Result<Self::Output, Self::Error>>;
    type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        self.inner.listen_on(addr)
    }

    // dials via Tor's socks5 proxy if configured and if the provided address is an
    // onion address. or it falls back to Tcp dialling
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        match to_address_string(addr.clone()) {
            Ok(tor_address_string) => Ok(async move {
                tracing::trace!("Connecting through Tor proxy to address: {}", addr);

                let sock = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.socks_port));
                let stream = Socks5Stream::connect(sock, tor_address_string)
                    .await
                    .map_err(|e| io::Error::new(io::ErrorKind::ConnectionRefused, e))?;

                tracing::trace!("Connection through Tor established");

                Ok(TcpStream(stream.into_inner()))
            }
            .boxed()),
            Err(error) => {
                tracing::warn!(
                    address = %addr,
                    "Address could not be formatted. Dialling via clear net. Error {:#}", error,
                );
                self.inner.dial(addr)
            }
        }
    }

    fn address_translation(&self, listen: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.address_translation(listen, observed)
    }
}

/// Tor expects an address format of ADDR:PORT.
/// This helper function tries to convert the provided multi-address into this
/// format. None is returned if an unsupported protocol was provided.
fn to_address_string(multi: Multiaddr) -> Result<String> {
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
        Some(Protocol::Ip4(addr)) => Some(format!("{}", addr)),
        Some(Protocol::Ip6(addr)) => Some(format!("{}", addr)),
        Some(Protocol::Dns(addr)) => Some(format!("{}", addr)),
        Some(Protocol::Dns4(addr)) => Some(format!("{}", addr)),
        _ => None,
    }
    .ok_or_else(|| {
        anyhow!(
            "Could not format address {}. Please consider reporting this issue. ",
            multi
        )
    })?;

    let port_string = match protocols.next() {
        Some(Protocol::Tcp(port)) => Some(format!("{}", port)),
        Some(Protocol::Udp(port)) => Some(format!("{}", port)),
        _ => None,
    };

    if let Some(port) = port_string {
        Ok(format!("{}:{}", address_string, port))
    } else {
        Ok(address_string)
    }
}

#[cfg(test)]
pub mod test {
    use crate::network::tor_transport::to_address_string;

    #[test]
    fn test_tor_address_string() {
        let address =
            "/onion3/oarchy4tamydxcitaki6bc2v4leza6v35iezmu2chg2bap63sv6f2did:1024/p2p/12D3KooWPD4uHN74SHotLN7VCH7Fm8zZgaNVymYcpeF1fpD2guc9"
            ;
        let address_string =
            to_address_string(address.parse().unwrap()).expect("To be a multi formatted address.");
        assert_eq!(
            address_string,
            "oarchy4tamydxcitaki6bc2v4leza6v35iezmu2chg2bap63sv6f2did.onion:1024"
        );
    }

    #[test]
    fn tcp_to_address_string_should_be_some() {
        let address = "/ip4/127.0.0.1/tcp/7777";
        let address_string =
            to_address_string(address.parse().unwrap()).expect("To be a formatted multi address. ");
        assert_eq!(address_string, "127.0.0.1:7777");
    }

    #[test]
    fn ip6_to_address_string_should_be_some() {
        let address = "/ip6/2001:db8:85a3:8d3:1319:8a2e:370:7348/tcp/7777";
        let address_string =
            to_address_string(address.parse().unwrap()).expect("To be a formatted multi address. ");
        assert_eq!(address_string, "2001:db8:85a3:8d3:1319:8a2e:370:7348:7777");
    }

    #[test]
    fn udp_to_address_string_should_be_some() {
        let address = "/ip4/127.0.0.1/udp/7777";
        let address_string =
            to_address_string(address.parse().unwrap()).expect("To be a formatted multi address. ");
        assert_eq!(address_string, "127.0.0.1:7777");
    }

    #[test]
    fn ws_to_address_string_should_be_some() {
        let address = "/ip4/127.0.0.1/tcp/7777/ws";
        let address_string =
            to_address_string(address.parse().unwrap()).expect("To be a formatted multi address. ");
        assert_eq!(address_string, "127.0.0.1:7777");
    }

    #[test]
    fn dns4_to_address_string_should_be_some() {
        let address = "/dns4/randomdomain.com/tcp/7777";
        let address_string =
            to_address_string(address.parse().unwrap()).expect("To be a formatted multi address. ");
        assert_eq!(address_string, "randomdomain.com:7777");
    }

    #[test]
    fn dns_to_address_string_should_be_some() {
        let address = "/dns/randomdomain.com/tcp/7777";
        let address_string =
            to_address_string(address.parse().unwrap()).expect("To be a formatted multi address. ");
        assert_eq!(address_string, "randomdomain.com:7777");
    }

    #[test]
    fn dnsaddr_to_address_string_should_be_none() {
        let address = "/dnsaddr/randomdomain.com";
        let address_string = to_address_string(address.parse().unwrap()).ok();
        assert_eq!(address_string, None);
    }
}
