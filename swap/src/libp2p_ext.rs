use libp2p::multiaddr::Protocol;
use libp2p::{Multiaddr, PeerId};

pub trait MultiAddrExt {
    fn extract_peer_id(&self) -> Option<PeerId>;
    fn split_peer_id(&self) -> Option<(PeerId, Multiaddr)>;
}

impl MultiAddrExt for Multiaddr {
    fn extract_peer_id(&self) -> Option<PeerId> {
        match self.iter().last()? {
            Protocol::P2p(peer_id) => Some(peer_id),
            _ => None,
        }
    }

    // Takes a peer id like /ip4/192.168.178.64/tcp/9939/p2p/12D3KooWQsqsCyJ9ae1YEAJZAfoVdVFZdDdUq3yvZ92btq7hSv9f
    // and returns the peer id and the original address *with* the peer id
    fn split_peer_id(&self) -> Option<(PeerId, Multiaddr)> {
        let peer_id = self.extract_peer_id()?;
        let address = self.clone();
        Some((peer_id, address))
    }
}
