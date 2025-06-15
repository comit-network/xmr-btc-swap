pub mod api;
mod behaviour;
pub mod cancel_and_refund;
pub mod command;
mod event_loop;
mod list_sellers;
pub mod transport;
pub mod watcher;

pub use behaviour::{Behaviour, OutEvent};
pub use cancel_and_refund::{cancel, cancel_and_refund, refund};
pub use event_loop::{EventLoop, EventLoopHandle};
pub use list_sellers::{list_sellers, SellerStatus};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asb;
    use crate::asb::rendezvous::RendezvousNode;
    use crate::cli::list_sellers::{QuoteWithAddress, SellerStatus};
    use crate::network::quote;
    use crate::network::quote::BidQuote;
    use crate::network::rendezvous::XmrBtcNamespace;
    use crate::network::test::{new_swarm, SwarmExt};
    use futures::StreamExt;
    use libp2p::core::Endpoint;
    use libp2p::multiaddr::Protocol;
    use libp2p::swarm::{
        ConnectionDenied, ConnectionId, FromSwarm, THandlerInEvent, THandlerOutEvent, ToSwarm,
    };
    use libp2p::{identity, rendezvous, request_response, Multiaddr, PeerId};
    use semver::Version;
    use std::collections::HashSet;
    use std::task::Poll;
    use std::time::Duration;

    // Test-only struct for compatibility
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    struct Seller {
        multiaddr: Multiaddr,
        status: SellerStatus,
    }

    #[tokio::test]
    #[ignore]
    // Due to an issue with the libp2p rendezvous library
    // This needs to be fixed upstream and was
    // introduced in our codebase by a libp2p refactor which bumped the version of libp2p:
    //
    // - The new bumped rendezvous client works, and can connect to an old rendezvous server
    // - The new rendezvous has an issue, which is why these test (use the new mock server)
    //   do not work
    //
    // Ignore this test for now . This works in production :)
    async fn list_sellers_should_report_all_registered_asbs_with_a_quote() {
        let namespace = XmrBtcNamespace::Mainnet;
        let (rendezvous_address, rendezvous_peer_id) = setup_rendezvous_point().await;
        let expected_seller_1 = setup_asb(rendezvous_peer_id, &rendezvous_address, namespace).await;
        let expected_seller_2 = setup_asb(rendezvous_peer_id, &rendezvous_address, namespace).await;

        let list_sellers = list_sellers(
            vec![(rendezvous_peer_id, rendezvous_address)],
            namespace,
            None,
            identity::Keypair::generate_ed25519(),
            None,
            None,
        );
        let sellers = tokio::time::timeout(Duration::from_secs(15), list_sellers)
            .await
            .unwrap()
            .unwrap();

        // Convert SellerStatus to test Seller struct
        let actual_sellers: Vec<Seller> = sellers
            .into_iter()
            .map(|status| Seller {
                multiaddr: match &status {
                    SellerStatus::Online(quote_with_addr) => quote_with_addr.multiaddr.clone(),
                    SellerStatus::Unreachable(_) => "/ip4/0.0.0.0/tcp/0".parse().unwrap(), // placeholder
                },
                status,
            })
            .collect();

        assert_eq!(
            HashSet::<Seller>::from_iter(actual_sellers),
            HashSet::<Seller>::from_iter([expected_seller_1, expected_seller_2])
        )
    }

    async fn setup_rendezvous_point() -> (Multiaddr, PeerId) {
        let mut rendezvous_node = new_swarm(|_| RendezvousPointBehaviour::default());
        let rendezvous_address = rendezvous_node.listen_on_tcp_localhost().await;
        let rendezvous_peer_id = *rendezvous_node.local_peer_id();

        tokio::spawn(async move {
            loop {
                rendezvous_node.next().await;
            }
        });

        (rendezvous_address, rendezvous_peer_id)
    }

    async fn setup_asb(
        rendezvous_peer_id: PeerId,
        rendezvous_address: &Multiaddr,
        namespace: XmrBtcNamespace,
    ) -> Seller {
        let static_quote = BidQuote {
            price: bitcoin::Amount::from_sat(1337),
            min_quantity: bitcoin::Amount::from_sat(42),
            max_quantity: bitcoin::Amount::from_sat(9001),
        };

        let mut asb = new_swarm(|identity| {
            let rendezvous_node =
                RendezvousNode::new(rendezvous_address, rendezvous_peer_id, namespace, None);
            let rendezvous = asb::rendezvous::Behaviour::new(identity, vec![rendezvous_node]);

            StaticQuoteAsbBehaviour {
                inner: StaticQuoteAsbBehaviourInner {
                    rendezvous,
                    quote: quote::asb(),
                },
                static_quote,
                registered: false,
            }
        });

        let asb_address = asb.listen_on_tcp_localhost().await;
        asb.add_external_address(asb_address.clone());

        let asb_peer_id = *asb.local_peer_id();

        // avoid race condition where `list_sellers` tries to discover before we are
        // registered block this function until we are registered
        while !asb.behaviour().registered {
            asb.next().await;
        }

        tokio::spawn(async move {
            loop {
                asb.next().await;
            }
        });

        let full_address = asb_address.with(Protocol::P2p(asb_peer_id));
        Seller {
            multiaddr: full_address.clone(),
            status: SellerStatus::Online(QuoteWithAddress {
                multiaddr: full_address,
                peer_id: asb_peer_id,
                quote: static_quote,
                version: Version::parse("1.0.0").unwrap(),
            }),
        }
    }

    #[derive(libp2p::swarm::NetworkBehaviour)]
    struct StaticQuoteAsbBehaviourInner {
        rendezvous: asb::rendezvous::Behaviour,
        quote: quote::Behaviour,
    }

    struct StaticQuoteAsbBehaviour {
        inner: StaticQuoteAsbBehaviourInner,
        static_quote: BidQuote,
        registered: bool,
    }

    impl libp2p::swarm::NetworkBehaviour for StaticQuoteAsbBehaviour {
        type ConnectionHandler =
            <StaticQuoteAsbBehaviourInner as libp2p::swarm::NetworkBehaviour>::ConnectionHandler;
        type ToSwarm = <StaticQuoteAsbBehaviourInner as libp2p::swarm::NetworkBehaviour>::ToSwarm;

        fn handle_established_inbound_connection(
            &mut self,
            connection_id: ConnectionId,
            peer: PeerId,
            local_addr: &Multiaddr,
            remote_addr: &Multiaddr,
        ) -> Result<libp2p::swarm::THandler<Self>, ConnectionDenied> {
            self.inner.handle_established_inbound_connection(
                connection_id,
                peer,
                local_addr,
                remote_addr,
            )
        }

        fn handle_established_outbound_connection(
            &mut self,
            connection_id: ConnectionId,
            peer: PeerId,
            addr: &Multiaddr,
            role_override: Endpoint,
        ) -> Result<libp2p::swarm::THandler<Self>, ConnectionDenied> {
            self.inner.handle_established_outbound_connection(
                connection_id,
                peer,
                addr,
                role_override,
            )
        }

        fn on_swarm_event(&mut self, event: FromSwarm<'_>) {
            self.inner.on_swarm_event(event);
        }

        fn on_connection_handler_event(
            &mut self,
            peer_id: PeerId,
            connection_id: ConnectionId,
            event: THandlerOutEvent<Self>,
        ) {
            self.inner
                .on_connection_handler_event(peer_id, connection_id, event);
        }

        fn poll(
            &mut self,
            cx: &mut std::task::Context<'_>,
        ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
            match self.inner.poll(cx) {
                Poll::Ready(ToSwarm::GenerateEvent(event)) => match event {
                    StaticQuoteAsbBehaviourInnerEvent::Rendezvous(rendezvous_event) => {
                        if let rendezvous::client::Event::Registered { .. } = rendezvous_event {
                            self.registered = true;
                        }

                        Poll::Ready(ToSwarm::GenerateEvent(
                            StaticQuoteAsbBehaviourInnerEvent::Rendezvous(rendezvous_event),
                        ))
                    }
                    StaticQuoteAsbBehaviourInnerEvent::Quote(quote_event) => {
                        if let request_response::Event::Message {
                            message: quote::Message::Request { channel, .. },
                            ..
                        } = quote_event
                        {
                            self.inner
                                .quote
                                .send_response(channel, self.static_quote)
                                .unwrap();

                            return Poll::Pending;
                        }

                        Poll::Ready(ToSwarm::GenerateEvent(
                            StaticQuoteAsbBehaviourInnerEvent::Quote(quote_event),
                        ))
                    }
                },
                other => other,
            }
        }
    }

    #[derive(libp2p::swarm::NetworkBehaviour)]
    struct RendezvousPointBehaviour {
        rendezvous: rendezvous::server::Behaviour,
    }

    impl Default for RendezvousPointBehaviour {
        fn default() -> Self {
            RendezvousPointBehaviour {
                rendezvous: rendezvous::server::Behaviour::new(
                    rendezvous::server::Config::default(),
                ),
            }
        }
    }
}
