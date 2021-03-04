use anyhow::Result;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::Boxed;
use libp2p::core::upgrade::{SelectUpgrade, Version};
use libp2p::core::{identity, Transport};
use libp2p::dns::DnsConfig;
use libp2p::mplex::MplexConfig;
use libp2p::noise::{self, NoiseConfig, X25519Spec};
use libp2p::{yamux, PeerId};

/// Builds a libp2p transport with the following features:
/// - TcpConnection
/// - DNS name resolution
/// - authentication via noise
/// - multiplexing via yamux or mplex
pub fn build(id_keys: &identity::Keypair) -> Result<SwapTransport> {
    use libp2p::tcp::TokioTcpConfig;

    let dh_keys = noise::Keypair::<X25519Spec>::new().into_authentic(id_keys)?;
    let noise = NoiseConfig::xx(dh_keys).into_authenticated();

    let tcp = TokioTcpConfig::new().nodelay(true);
    let dns = DnsConfig::new(tcp)?;

    let transport = dns
        .upgrade(Version::V1)
        .authenticate(noise)
        .multiplex(SelectUpgrade::new(
            yamux::YamuxConfig::default(),
            MplexConfig::new(),
        ))
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    Ok(transport)
}

pub type SwapTransport = Boxed<(PeerId, StreamMuxerBox)>;
