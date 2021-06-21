use anyhow::Result;
use futures::{AsyncRead, AsyncWrite};
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::Boxed;
use libp2p::core::upgrade::{SelectUpgrade, Version};
use libp2p::mplex::MplexConfig;
use libp2p::noise::{self, NoiseConfig, X25519Spec};
use libp2p::{identity, yamux, PeerId, Transport};
use std::time::Duration;

/// "Completes" a transport by applying the authentication and multiplexing
/// upgrades.
///
/// Even though the actual transport technology in use might be different, for
/// two libp2p applications to be compatible, the authentication and
/// multiplexing upgrades need to be compatible.
pub fn authenticate_and_multiplex<T>(
    transport: Boxed<T>,
    identity: &identity::Keypair,
) -> Result<Boxed<(PeerId, StreamMuxerBox)>>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let auth_upgrade = {
        let noise_identity = noise::Keypair::<X25519Spec>::new().into_authentic(identity)?;
        NoiseConfig::xx(noise_identity).into_authenticated()
    };
    let multiplex_upgrade = SelectUpgrade::new(yamux::YamuxConfig::default(), MplexConfig::new());

    let transport = transport
        .upgrade(Version::V1)
        .authenticate(auth_upgrade)
        .multiplex(multiplex_upgrade)
        .timeout(Duration::from_secs(20))
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    Ok(transport)
}
