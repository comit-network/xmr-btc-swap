use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::{fmt, io};

use libp2p::tcp::tokio::TcpStream;
use tokio_socks::tcp::Socks5Stream;

pub mod dial_only;
pub mod duplex;
mod fmt_as_tor_compatible_address;
pub mod torut_ext;

async fn dial_via_tor(onion_address: String, socks_port: u16) -> anyhow::Result<TcpStream, Error> {
    let sock = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, socks_port));
    let stream = Socks5Stream::connect(sock, onion_address)
        .await
        .map_err(Error::UnreachableProxy)?;
    let stream = TcpStream(stream.into_inner());

    Ok(stream)
}

#[derive(Debug)]
pub enum Error {
    OnlyWildcardAllowed,
    Torut(torut_ext::Error),
    UnreachableProxy(tokio_socks::Error),
    InnerTransprot(io::Error),
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl From<torut_ext::Error> for Error {
    fn from(e: torut_ext::Error) -> Self {
        Error::Torut(e)
    }
}
