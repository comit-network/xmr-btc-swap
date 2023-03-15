use crate::network::quote::BidQuote;
use crate::network::rendezvous::XmrBtcNamespace;
use crate::network::{quote, swarm};
use anyhow::{Context, Result};
use futures::StreamExt;
use libp2p::multiaddr::Protocol;
use libp2p::ping::{Ping, PingConfig, PingEvent};
use libp2p::request_response::{RequestResponseEvent, RequestResponseMessage};
use libp2p::swarm::dial_opts::DialOpts;
use libp2p::swarm::SwarmEvent;
use libp2p::{identity, rendezvous, Multiaddr, PeerId, Swarm};
use serde::Serialize;
use serde_with::{serde_as, DisplayFromStr};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::time::Duration;

/// Returns sorted list of sellers, with [Online](Status::Online) listed first.
///
/// First uses the rendezvous node to discover peers in the given namespace,
/// then fetches a quote from each peer that was discovered. If fetching a quote
/// from a discovered peer fails the seller's status will be
/// [Unreachable](Status::Unreachable).
pub async fn list_sellers(
    rendezvous_node_peer_id: PeerId,
    rendezvous_node_addr: Multiaddr,
    namespace: XmrBtcNamespace,
    tor_socks5_port: u16,
    identity: identity::Keypair,
) -> Result<Vec<Seller>> {
    let behaviour = Behaviour {
        rendezvous: rendezvous::client::Behaviour::new(identity.clone()),
        quote: quote::cli(),
        ping: Ping::new(
            PingConfig::new()
                .with_keep_alive(false)
                .with_interval(Duration::from_secs(86_400)),
        ),
    };
    let mut swarm = swarm::cli(identity, tor_socks5_port, behaviour).await?;

    swarm
        .behaviour_mut()
        .quote
        .add_address(&rendezvous_node_peer_id, rendezvous_node_addr.clone());

    swarm
        .dial(DialOpts::from(rendezvous_node_peer_id))
        .context("Failed to dial rendezvous node")?;

    let event_loop = EventLoop::new(
        swarm,
        rendezvous_node_peer_id,
        rendezvous_node_addr,
        namespace,
    );
    let sellers = event_loop.run().await;

    Ok(sellers)
}

#[serde_as]
#[derive(Debug, Serialize, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct Seller {
    pub status: Status,
    #[serde_as(as = "DisplayFromStr")]
    pub multiaddr: Multiaddr,
}

#[derive(Debug, Serialize, PartialEq, Eq, Hash, Copy, Clone, Ord, PartialOrd)]
pub enum Status {
    Online(BidQuote),
    Unreachable,
}

#[derive(Debug)]
enum OutEvent {
    Rendezvous(rendezvous::client::Event),
    Quote(quote::OutEvent),
    Ping(PingEvent),
}

impl From<rendezvous::client::Event> for OutEvent {
    fn from(event: rendezvous::client::Event) -> Self {
        OutEvent::Rendezvous(event)
    }
}

impl From<quote::OutEvent> for OutEvent {
    fn from(event: quote::OutEvent) -> Self {
        OutEvent::Quote(event)
    }
}

#[derive(libp2p::NetworkBehaviour)]
#[behaviour(event_process = false)]
#[behaviour(out_event = "OutEvent")]
struct Behaviour {
    rendezvous: rendezvous::client::Behaviour,
    quote: quote::Behaviour,
    ping: Ping,
}

#[derive(Debug)]
enum QuoteStatus {
    Pending,
    Received(Status),
}

#[derive(Debug)]
enum State {
    WaitForDiscovery,
    WaitForQuoteCompletion,
}

struct EventLoop {
    swarm: Swarm<Behaviour>,
    rendezvous_peer_id: PeerId,
    rendezvous_addr: Multiaddr,
    namespace: XmrBtcNamespace,
    reachable_asb_address: HashMap<PeerId, Multiaddr>,
    unreachable_asb_address: HashMap<PeerId, Multiaddr>,
    asb_quote_status: HashMap<PeerId, QuoteStatus>,
    state: State,
}

impl EventLoop {
    fn new(
        swarm: Swarm<Behaviour>,
        rendezvous_peer_id: PeerId,
        rendezvous_addr: Multiaddr,
        namespace: XmrBtcNamespace,
    ) -> Self {
        Self {
            swarm,
            rendezvous_peer_id,
            rendezvous_addr,
            namespace,
            reachable_asb_address: Default::default(),
            unreachable_asb_address: Default::default(),
            asb_quote_status: Default::default(),
            state: State::WaitForDiscovery,
        }
    }

    async fn run(mut self) -> Vec<Seller> {
        loop {
            tokio::select! {
                swarm_event = self.swarm.select_next_some() => {
                    match swarm_event {
                        SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                            if peer_id == self.rendezvous_peer_id {
                                tracing::info!(
                                    "Connected to rendezvous point, discovering nodes in '{}' namespace ...",
                                    self.namespace
                                );

                                self.swarm.behaviour_mut().rendezvous.discover(
                                    Some(rendezvous::Namespace::new(self.namespace.to_string()).expect("our namespace to be a correct string")),
                                    None,
                                    None,
                                    self.rendezvous_peer_id,
                                );
                            } else {
                                let address = endpoint.get_remote_address();
                                tracing::debug!(%peer_id, %address, "Connection established to peer");
                                self.reachable_asb_address.insert(peer_id, address.clone());
                            }
                        }
                        SwarmEvent::OutgoingConnectionError { peer_id, error } => {
                            if let Some(peer_id) = peer_id {
                                if peer_id == self.rendezvous_peer_id {
                                    tracing::error!(
                                        %peer_id,
                                        %self.rendezvous_addr,
                                        "Failed to connect to rendezvous point: {}",
                                        error
                                    );

                                    // if the rendezvous node is unreachable we just stop
                                    return Vec::new();
                                } else {
                                    tracing::error!(
                                        %peer_id,
                                        "Failed to connect to peer: {}",
                                        error
                                    );
                                    self.unreachable_asb_address.insert(peer_id, Multiaddr::empty());

                                    match self.asb_quote_status.entry(peer_id) {
                                        Entry::Occupied(mut entry) => {
                                            entry.insert(QuoteStatus::Received(Status::Unreachable));
                                        },
                                        _ => {
                                            tracing::debug!(%peer_id, %error, "Connection error with unexpected peer");
                                        }
                                    }
                                }
                            } else {
                                tracing::debug!("Failed to connect (no peer id): {}", error);
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::Rendezvous(
                                                  libp2p::rendezvous::client::Event::Discovered { registrations, .. },
                                              )) => {
                            self.state = State::WaitForQuoteCompletion;

                            for registration in registrations {
                                let peer = registration.record.peer_id();
                                for address in registration.record.addresses() {
                                    tracing::info!(peer_id=%peer, address=%address, "Discovered peer");

                                    let p2p_suffix = Protocol::P2p(*peer.as_ref());
                                    let _address_with_p2p = if !address
                                        .ends_with(&Multiaddr::empty().with(p2p_suffix.clone()))
                                    {
                                        address.clone().with(p2p_suffix)
                                    } else {
                                        address.clone()
                                    };

                                    self.asb_quote_status.insert(peer, QuoteStatus::Pending);

                                    // add all external addresses of that peer to the quote behaviour
                                    self.swarm.behaviour_mut().quote.add_address(&peer, address.clone());
                                }

                                // request the quote, if we are not connected to the peer it will be dialed automatically
                                let _request_id = self.swarm.behaviour_mut().quote.send_request(&peer, ());
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::Quote(quote_response)) => {
                            match quote_response {
                                RequestResponseEvent::Message { peer, message } => {
                                    match message {
                                        RequestResponseMessage::Response { response, .. } => {
                                            if self.asb_quote_status.insert(peer, QuoteStatus::Received(Status::Online(response))).is_none() {
                                                tracing::error!(%peer, "Received bid quote from unexpected peer, this record will be removed!");
                                                self.asb_quote_status.remove(&peer);
                                            }
                                        }
                                        RequestResponseMessage::Request { .. } => unreachable!()
                                    }
                                }
                                RequestResponseEvent::OutboundFailure { peer, error, .. } => {
                                    if peer == self.rendezvous_peer_id {
                                        tracing::debug!(%peer, "Outbound failure when communicating with rendezvous node: {:#}", error);
                                    } else {
                                        tracing::debug!(%peer, "Ignoring seller, because unable to request quote: {:#}", error);
                                        self.asb_quote_status.remove(&peer);
                                    }
                                }
                                RequestResponseEvent::InboundFailure { peer, error, .. } => {
                                    if peer == self.rendezvous_peer_id {
                                        tracing::debug!(%peer, "Inbound failure when communicating with rendezvous node: {:#}", error);
                                    } else {
                                        tracing::debug!(%peer, "Ignoring seller, because unable to request quote: {:#}", error);
                                        self.asb_quote_status.remove(&peer);
                                    }
                                },
                                RequestResponseEvent::ResponseSent { .. } => unreachable!()
                            }
                        }
                        _ => {}
                    }
                }
            }

            match self.state {
                State::WaitForDiscovery => {
                    continue;
                }
                State::WaitForQuoteCompletion => {
                    let all_quotes_fetched = self
                        .asb_quote_status
                        .iter()
                        .map(|(peer_id, quote_status)| match quote_status {
                            QuoteStatus::Pending => Err(StillPending {}),
                            QuoteStatus::Received(Status::Online(quote)) => {
                                let address = self
                                    .reachable_asb_address
                                    .get(peer_id)
                                    .expect("if we got a quote we must have stored an address");

                                Ok(Seller {
                                    multiaddr: address.clone(),
                                    status: Status::Online(*quote),
                                })
                            }
                            QuoteStatus::Received(Status::Unreachable) => {
                                let address = self
                                    .unreachable_asb_address
                                    .get(peer_id)
                                    .expect("if we got a quote we must have stored an address");

                                Ok(Seller {
                                    multiaddr: address.clone(),
                                    status: Status::Unreachable,
                                })
                            }
                        })
                        .collect::<Result<Vec<_>, _>>();

                    match all_quotes_fetched {
                        Ok(mut sellers) => {
                            sellers.sort();
                            break sellers;
                        }
                        Err(StillPending {}) => continue,
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
struct StillPending {}

impl From<PingEvent> for OutEvent {
    fn from(event: PingEvent) -> Self {
        OutEvent::Ping(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sellers_sort_with_unreachable_coming_last() {
        let mut list = vec![
            Seller {
                multiaddr: "/ip4/127.0.0.1/tcp/1234".parse().unwrap(),
                status: Status::Unreachable,
            },
            Seller {
                multiaddr: Multiaddr::empty(),
                status: Status::Unreachable,
            },
            Seller {
                multiaddr: "/ip4/127.0.0.1/tcp/5678".parse().unwrap(),
                status: Status::Online(BidQuote {
                    price: Default::default(),
                    min_quantity: Default::default(),
                    max_quantity: Default::default(),
                }),
            },
        ];

        list.sort();

        assert_eq!(
            list,
            vec![
                Seller {
                    multiaddr: "/ip4/127.0.0.1/tcp/5678".parse().unwrap(),
                    status: Status::Online(BidQuote {
                        price: Default::default(),
                        min_quantity: Default::default(),
                        max_quantity: Default::default(),
                    })
                },
                Seller {
                    multiaddr: Multiaddr::empty(),
                    status: Status::Unreachable
                },
                Seller {
                    multiaddr: "/ip4/127.0.0.1/tcp/1234".parse().unwrap(),
                    status: Status::Unreachable
                },
            ]
        )
    }
}
