use crate::network::tor_transport::TorDialOnlyTransport;
use crate::network::transport::authenticate_and_multiplex;
use anyhow::Result;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::{Boxed, OptionalTransport};
use libp2p::dns::TokioDnsConfig;
use libp2p::tcp::TokioTcpConfig;
use libp2p::{identity, PeerId, Transport};

/// Creates the libp2p transport for the swap CLI.
///
/// The CLI's transport needs the following capabilities:
/// - Establish TCP connections
/// - Resolve DNS entries
/// - Dial onion-addresses through a running Tor daemon by connecting to the
///   socks5 port. If the port is not given, we will fall back to the regular
///   TCP transport.
pub fn new(
    identity: &identity::Keypair,
    maybe_tor_socks5_port: Option<u16>,
) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
    let tcp = TokioTcpConfig::new().nodelay(true);
    let tcp_with_dns = TokioDnsConfig::system(tcp)?;
    let maybe_tor_transport = match maybe_tor_socks5_port {
        Some(port) => OptionalTransport::some(TorDialOnlyTransport::new(port)),
        None => OptionalTransport::none(),
    };

    let transport = maybe_tor_transport.or_transport(tcp_with_dns).boxed();

    authenticate_and_multiplex(transport, identity)
}
