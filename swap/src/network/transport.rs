use crate::network::tor_transport::TorDialOnlyTransport;
use anyhow::Result;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::Boxed;
use libp2p::core::upgrade::{SelectUpgrade, Version};
use libp2p::dns::TokioDnsConfig;
use libp2p::mplex::MplexConfig;
use libp2p::noise::{self, NoiseConfig, X25519Spec};
use libp2p::tcp::TokioTcpConfig;
use libp2p::websocket::WsConfig;
use libp2p::{identity, yamux, PeerId, Transport};
use std::time::Duration;

/// Builds a libp2p transport with the following features:
/// - TcpConnection
/// - WebSocketConnection
/// - DNS name resolution
/// - authentication via noise
/// - multiplexing via yamux or mplex
pub fn build_clear_net(id_keys: &identity::Keypair) -> Result<SwapTransport> {
    let dh_keys = noise::Keypair::<X25519Spec>::new().into_authentic(id_keys)?;
    let noise = NoiseConfig::xx(dh_keys).into_authenticated();

    let tcp = TokioTcpConfig::new().nodelay(true);
    let dns = TokioDnsConfig::system(tcp)?;
    let websocket = WsConfig::new(dns.clone());

    let transport = websocket
        .or_transport(dns)
        .upgrade(Version::V1)
        .authenticate(noise)
        .multiplex(SelectUpgrade::new(
            yamux::YamuxConfig::default(),
            MplexConfig::new(),
        ))
        .timeout(Duration::from_secs(20))
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    Ok(transport)
}

/// Builds a libp2p transport with the following features:
/// - TorTcpConnection
/// - WebSocketConnection
/// - DNS name resolution
/// - authentication via noise
/// - multiplexing via yamux or mplex
pub fn build_tor(id_keys: &identity::Keypair, tor_socks5_port: u16) -> Result<SwapTransport> {
    let dh_keys = noise::Keypair::<X25519Spec>::new().into_authentic(id_keys)?;
    let noise = NoiseConfig::xx(dh_keys).into_authenticated();

    let tcp = TokioTcpConfig::new().nodelay(true);
    let tcp_with_dns = TokioDnsConfig::system(tcp)?;
    let websocket_with_dns = WsConfig::new(tcp_with_dns.clone());
    let tor_dial_only = TorDialOnlyTransport::new(tor_socks5_port);

    let transport = tor_dial_only
        .or_transport(tcp_with_dns)
        .or_transport(websocket_with_dns)
        .upgrade(Version::V1)
        .authenticate(noise)
        .multiplex(SelectUpgrade::new(
            yamux::YamuxConfig::default(),
            MplexConfig::new(),
        ))
        .timeout(Duration::from_secs(20))
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    Ok(transport)
}

pub type SwapTransport = Boxed<(PeerId, StreamMuxerBox)>;
