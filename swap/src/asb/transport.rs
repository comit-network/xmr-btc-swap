use crate::network::transport::authenticate_and_multiplex;
use anyhow::Result;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::Boxed;
use libp2p::dns::TokioDnsConfig;
use libp2p::tcp::TokioTcpConfig;
use libp2p::websocket::WsConfig;
use libp2p::{identity, PeerId, Transport};

/// Creates the libp2p transport for the ASB.
pub fn new(identity: &identity::Keypair) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
    let tcp = TokioTcpConfig::new().nodelay(true);
    let tcp_with_dns = TokioDnsConfig::system(tcp)?;
    let websocket_with_dns = WsConfig::new(tcp_with_dns.clone());

    let transport = tcp_with_dns.or_transport(websocket_with_dns).boxed();

    authenticate_and_multiplex(transport, identity)
}
