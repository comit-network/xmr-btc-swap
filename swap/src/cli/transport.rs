use std::sync::Arc;

use crate::network::transport::authenticate_and_multiplex;
use anyhow::Result;
use arti_client::TorClient;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::{Boxed, OptionalTransport};
use libp2p::dns;
use libp2p::tcp;
use libp2p::{identity, PeerId, Transport};
use libp2p_community_tor::{AddressConversion, TorTransport};
use tor_rtcompat::tokio::TokioRustlsRuntime;

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
    maybe_tor_client: Option<Arc<TorClient<TokioRustlsRuntime>>>,
) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
    let tcp = tcp::tokio::Transport::new(tcp::Config::new().nodelay(true));
    let tcp_with_dns = dns::tokio::Transport::system(tcp)?;

    let maybe_tor_transport: OptionalTransport<TorTransport> = match maybe_tor_client {
        Some(client) => OptionalTransport::some(libp2p_community_tor::TorTransport::from_client(
            client,
            AddressConversion::IpAndDns,
        )),
        None => OptionalTransport::none(),
    };

    let transport = maybe_tor_transport.or_transport(tcp_with_dns).boxed();

    authenticate_and_multiplex(transport, identity)
}
