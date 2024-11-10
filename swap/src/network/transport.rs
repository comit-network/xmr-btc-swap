use anyhow::Result;
use futures::{AsyncRead, AsyncWrite};
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::Boxed;
use libp2p::core::upgrade::Version;
use libp2p::noise;
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
    let auth_upgrade = noise::Config::new(identity)?;
    let multiplex_upgrade = yamux::Config::default();

    let transport = transport
        .upgrade(Version::V1)
        .authenticate(auth_upgrade)
        .multiplex(multiplex_upgrade)
        .timeout(Duration::from_secs(20))
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    Ok(transport)
}
