use anyhow::Result;
use libp2p::{
    core::{
        identity,
        muxing::StreamMuxerBox,
        transport::Boxed,
        upgrade::{SelectUpgrade, Version},
        Multiaddr, Transport,
    },
    dns::DnsConfig,
    mplex::MplexConfig,
    noise::{self, NoiseConfig, X25519Spec},
    yamux, PeerId,
};

/// Builds a libp2p transport without Tor with the following features:
/// - TcpConnection
/// - DNS name resolution
/// - authentication via noise
/// - multiplexing via yamux or mplex
pub fn build(id_keys: identity::Keypair) -> Result<SwapTransport> {
    use libp2p::tcp::TokioTcpConfig;

    let dh_keys = noise::Keypair::<X25519Spec>::new().into_authentic(&id_keys)?;
    let noise = NoiseConfig::xx(dh_keys).into_authenticated();

    let tcp = TokioTcpConfig::new().nodelay(true);
    let dns = DnsConfig::new(tcp)?;

    let transport = dns
        .upgrade(Version::V1)
        .authenticate(noise)
        .multiplex(SelectUpgrade::new(
            yamux::Config::default(),
            MplexConfig::new(),
        ))
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    Ok(transport)
}
/// Builds a libp2p transport with Tor and with the following features:
/// - TCP connection over the Tor network
/// - DNS name resolution
/// - authentication via noise
/// - multiplexing via yamux or mplex
pub fn build_tor(
    id_keys: identity::Keypair,
    addr: libp2p::core::Multiaddr,
    port: u16,
) -> Result<SwapTransport> {
    use libp2p_tokio_socks5::Socks5TokioTcpConfig;
    use std::collections::HashMap;

    let map: HashMap<Multiaddr, u16> = [(addr, port)].iter().cloned().collect();

    let dh_keys = noise::Keypair::<X25519Spec>::new().into_authentic(&id_keys)?;
    let noise = NoiseConfig::xx(dh_keys).into_authenticated();

    let socks = Socks5TokioTcpConfig::default().nodelay(true).onion_map(map);
    let dns = DnsConfig::new(socks)?;

    let transport = dns
        .upgrade(Version::V1)
        .authenticate(noise)
        .multiplex(SelectUpgrade::new(
            yamux::Config::default(),
            MplexConfig::new(),
        ))
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    Ok(transport)
}

pub type SwapTransport = Boxed<(PeerId, StreamMuxerBox)>;
