use crate::network::quote::BidQuote;
use crate::network::{quote, swarm};
use crate::rendezvous::XmrBtcNamespace;
use anyhow::Result;
use futures::StreamExt;
use libp2p::multiaddr::Protocol;
use libp2p::rendezvous::{Namespace, Rendezvous};
use libp2p::request_response::{RequestResponseEvent, RequestResponseMessage};
use libp2p::swarm::SwarmEvent;
use libp2p::{identity, rendezvous, Multiaddr, PeerId, Swarm};
use std::collections::HashMap;

pub async fn list_sellers(
    rendezvous_node_peer_id: PeerId,
    rendezvous_node_addr: Multiaddr,
    namespace: XmrBtcNamespace,
    tor_socks5_port: u16,
    identity: identity::Keypair,
) -> Result<Vec<Seller>> {
    let behaviour = Behaviour {
        rendezvous: Rendezvous::new(identity.clone(), rendezvous::Config::default()),
        quote: quote::cli(),
    };
    let mut swarm = swarm::cli(identity, tor_socks5_port, behaviour).await?;

    let _ = swarm.dial_addr(rendezvous_node_addr.clone());

    let event_loop = EventLoop::new(
        swarm,
        rendezvous_node_peer_id,
        rendezvous_node_addr,
        namespace,
    );
    let makers = event_loop.run().await;

    Ok(makers)
}

pub struct Seller {
    pub peer_id: PeerId,
    pub multiaddr: Multiaddr,
    pub quote: BidQuote,
}

#[derive(Debug)]
pub enum OutEvent {
    Rendezvous(rendezvous::Event),
    Quote(quote::OutEvent),
}

impl From<rendezvous::Event> for OutEvent {
    fn from(event: rendezvous::Event) -> Self {
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
pub struct Behaviour {
    pub rendezvous: Rendezvous,
    pub quote: quote::Behaviour,
}

#[derive(Debug)]
enum QuoteStatus {
    Pending,
    Received(BidQuote),
}

pub struct EventLoop {
    swarm: Swarm<Behaviour>,
    rendezvous_peer_id: PeerId,
    rendezvous_addr: Multiaddr,
    namespace: XmrBtcNamespace,
    asb_address: HashMap<PeerId, Multiaddr>,
    asb_quote_status: HashMap<PeerId, QuoteStatus>,
}

impl EventLoop {
    pub fn new(
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
            asb_address: Default::default(),
            asb_quote_status: Default::default(),
        }
    }

    pub async fn run(mut self) -> Vec<Seller> {
        loop {
            tokio::select! {
                swarm_event = self.swarm.select_next_some() => {
                    match swarm_event {
                        SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                            if peer_id == self.rendezvous_peer_id{
                                tracing::info!(
                                    "Connected to rendezvous point, discovering nodes in '{}' namespace ...",
                                    self.namespace
                                );

                                self.swarm.behaviour_mut().rendezvous.discover(
                                    Some(Namespace::new(self.namespace.to_string()).expect("our namespace to be a correct string")),
                                    None,
                                    None,
                                    self.rendezvous_peer_id,
                                );
                            } else {
                                let address = endpoint.get_remote_address();
                                self.asb_address.insert(peer_id, address.clone());
                            }
                        }
                        SwarmEvent::UnreachableAddr { error, address, .. }
                        | SwarmEvent::UnknownPeerUnreachableAddr { error, address, .. } => {
                            if address == self.rendezvous_addr {
                                tracing::error!(
                                    "Failed to connect to rendezvous point at {}: {}",
                                    address,
                                    error
                                );
                                todo!("Better error handling, return with error")
                            }
                        }
                        SwarmEvent::Behaviour(OutEvent::Rendezvous(
                                                  rendezvous::Event::Discovered { registrations, .. },
                                              )) => {
                            for registration in registrations {
                                let peer = registration.record.peer_id();
                                for address in registration.record.addresses() {
                                    tracing::info!("Discovered peer {} at {}", peer, address);

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
                                            if self.asb_quote_status.insert(peer, QuoteStatus::Received(response)).is_none() {
                                                tracing::error!(%peer, "Received bid quote from unexpected peer, this record will be removed!");
                                                self.asb_quote_status.remove(&peer);
                                            }
                                        }
                                        RequestResponseMessage::Request { .. } => unreachable!()
                                    }
                                }
                                RequestResponseEvent::OutboundFailure { peer, error, .. } => {
                                    tracing::debug!(%peer, "Ignoring seller, because unable to request quote: {:#}", error);
                                    self.asb_quote_status.remove(&peer);
                                }
                                RequestResponseEvent::InboundFailure { .. } => unreachable!(),
                                RequestResponseEvent::ResponseSent { .. } => unreachable!()
                            }
                        }
                        _ => {}
                    }
                }
            }

            let all_quotes_fetched = self
                .asb_quote_status
                .iter()
                .map(|(peer_id, quote_status)| match quote_status {
                    QuoteStatus::Pending => Err(StillPending {}),
                    QuoteStatus::Received(quote) => {
                        let address = self
                            .asb_address
                            .get(&peer_id)
                            .expect("if we got a quote we must have stored an address");

                        Ok(Seller {
                            peer_id: *peer_id,
                            multiaddr: address.clone(),
                            quote: *quote,
                        })
                    }
                })
                .collect::<Result<Vec<_>, _>>();

            match all_quotes_fetched {
                Ok(makers) => break makers,
                Err(StillPending {}) => continue,
            }
        }
    }
}

struct StillPending {}
