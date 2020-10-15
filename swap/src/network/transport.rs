use anyhow::Result;
use libp2p::{
    core::{
        either::EitherError,
        identity,
        muxing::StreamMuxerBox,
        transport::{boxed::Boxed, timeout::TransportTimeoutError},
        upgrade::{SelectUpgrade, Version},
        Transport, UpgradeError,
    },
    dns::{DnsConfig, DnsErr},
    mplex::MplexConfig,
    noise::{self, NoiseConfig, NoiseError, X25519Spec},
    tcp::TokioTcpConfig,
    yamux, PeerId,
};
use std::{io, time::Duration};

/// Builds a libp2p transport with the following features:
/// - TcpConnection
/// - DNS name resolution
/// - authentication via noise
/// - multiplexing via yamux or mplex
pub fn build(id_keys: identity::Keypair) -> Result<SwapTransport> {
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
        .timeout(Duration::from_secs(20))
        .boxed();

    Ok(transport)
}

pub type SwapTransport = Boxed<
    (PeerId, StreamMuxerBox),
    TransportTimeoutError<
        EitherError<
            EitherError<DnsErr<io::Error>, UpgradeError<NoiseError>>,
            UpgradeError<EitherError<io::Error, io::Error>>,
        >,
    >,
>;
