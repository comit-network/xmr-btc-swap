use crate::asb::event_loop::LatestRate;
use crate::env;
use crate::network::quote::BidQuote;
use crate::network::rendezvous::XmrBtcNamespace;
use crate::network::swap_setup::alice;
use crate::network::swap_setup::alice::WalletSnapshot;
use crate::network::transport::authenticate_and_multiplex;
use crate::network::{encrypted_signature, quote, transfer_proof};
use crate::protocol::alice::State3;
use anyhow::{anyhow, Error, Result};
use futures::FutureExt;
use libp2p::core::connection::ConnectionId;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::Boxed;
use libp2p::dns::TokioDnsConfig;
use libp2p::identify::{Identify, IdentifyConfig, IdentifyEvent};
use libp2p::ping::{Ping, PingConfig, PingEvent};
use libp2p::request_response::{RequestId, ResponseChannel};
use libp2p::swarm::dial_opts::PeerCondition;
use libp2p::swarm::{
    IntoProtocolsHandler, NetworkBehaviour, NetworkBehaviourAction, PollParameters,
    ProtocolsHandler,
};
use libp2p::tcp::TokioTcpConfig;
use libp2p::websocket::WsConfig;
use libp2p::{identity, Multiaddr, NetworkBehaviour, PeerId, Transport};
use std::task::Poll;
use std::time::Duration;
use uuid::Uuid;

pub mod transport {
    use super::*;

    /// Creates the libp2p transport for the ASB.
    pub fn new(identity: &identity::Keypair) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
        let tcp = TokioTcpConfig::new().nodelay(true);
        let tcp_with_dns = TokioDnsConfig::system(tcp)?;
        let websocket_with_dns = WsConfig::new(tcp_with_dns.clone());

        let transport = tcp_with_dns.or_transport(websocket_with_dns).boxed();

        authenticate_and_multiplex(transport, identity)
    }
}

pub mod behaviour {
    use super::*;

    #[allow(clippy::large_enum_variant)]
    #[derive(Debug)]
    pub enum OutEvent {
        SwapSetupInitiated {
            send_wallet_snapshot: bmrng::RequestReceiver<bitcoin::Amount, WalletSnapshot>,
        },
        SwapSetupCompleted {
            peer_id: PeerId,
            swap_id: Uuid,
            state3: State3,
        },
        SwapDeclined {
            peer: PeerId,
            error: alice::Error,
        },
        QuoteRequested {
            channel: ResponseChannel<BidQuote>,
            peer: PeerId,
        },
        TransferProofAcknowledged {
            peer: PeerId,
            id: RequestId,
        },
        EncryptedSignatureReceived {
            msg: encrypted_signature::Request,
            channel: ResponseChannel<()>,
            peer: PeerId,
        },
        Rendezvous(libp2p::rendezvous::client::Event),
        Failure {
            peer: PeerId,
            error: Error,
        },
        /// "Fallback" variant that allows the event mapping code to swallow
        /// certain events that we don't want the caller to deal with.
        Other,
    }

    impl OutEvent {
        pub fn unexpected_request(peer: PeerId) -> OutEvent {
            OutEvent::Failure {
                peer,
                error: anyhow!("Unexpected request received"),
            }
        }

        pub fn unexpected_response(peer: PeerId) -> OutEvent {
            OutEvent::Failure {
                peer,
                error: anyhow!("Unexpected response received"),
            }
        }
    }

    /// A `NetworkBehaviour` that represents an XMR/BTC swap node as Alice.
    #[derive(NetworkBehaviour)]
    #[behaviour(out_event = "OutEvent", event_process = false)]
    #[allow(missing_debug_implementations)]
    pub struct Behaviour<LR>
    where
        LR: LatestRate + Send + 'static,
    {
        pub rendezvous: libp2p::swarm::behaviour::toggle::Toggle<rendezous::Behaviour>,
        pub quote: quote::Behaviour,
        pub swap_setup: alice::Behaviour<LR>,
        pub transfer_proof: transfer_proof::Behaviour,
        pub encrypted_signature: encrypted_signature::Behaviour,
        pub identify: Identify,

        /// Ping behaviour that ensures that the underlying network connection
        /// is still alive. If the ping fails a connection close event
        /// will be emitted that is picked up as swarm event.
        ping: Ping,
    }

    impl<LR> Behaviour<LR>
    where
        LR: LatestRate + Send + 'static,
    {
        pub fn new(
            min_buy: bitcoin::Amount,
            max_buy: bitcoin::Amount,
            latest_rate: LR,
            resume_only: bool,
            env_config: env::Config,
            identify_params: (identity::Keypair, XmrBtcNamespace),
            rendezvous_params: Option<(identity::Keypair, PeerId, Multiaddr, XmrBtcNamespace)>,
        ) -> Self {
            let agentVersion = format!("asb/{} ({})", env!("CARGO_PKG_VERSION"), identify_params.1);
            let protocolVersion = "/comit/xmr/btc/1.0.0".to_string();
            let identifyConfig = IdentifyConfig::new(protocolVersion, identify_params.0.public())
                .with_agent_version(agentVersion);

            Self {
                rendezvous: libp2p::swarm::behaviour::toggle::Toggle::from(rendezvous_params.map(
                    |(identity, rendezvous_peer_id, rendezvous_address, namespace)| {
                        rendezous::Behaviour::new(
                            identity,
                            rendezvous_peer_id,
                            rendezvous_address,
                            namespace,
                            None, // use default ttl on rendezvous point
                        )
                    },
                )),
                quote: quote::asb(),
                swap_setup: alice::Behaviour::new(
                    min_buy,
                    max_buy,
                    env_config,
                    latest_rate,
                    resume_only,
                ),
                transfer_proof: transfer_proof::alice(),
                encrypted_signature: encrypted_signature::alice(),
                ping: Ping::new(PingConfig::new().with_keep_alive(true)),
                identify: Identify::new(identifyConfig),
            }
        }
    }

    impl From<PingEvent> for OutEvent {
        fn from(_: PingEvent) -> Self {
            OutEvent::Other
        }
    }

    impl From<IdentifyEvent> for OutEvent {
        fn from(_: IdentifyEvent) -> Self {
            OutEvent::Other
        }
    }

    impl From<libp2p::rendezvous::client::Event> for OutEvent {
        fn from(event: libp2p::rendezvous::client::Event) -> Self {
            OutEvent::Rendezvous(event)
        }
    }
}

pub mod rendezous {
    use super::*;
    use libp2p::swarm::dial_opts::DialOpts;
    use libp2p::swarm::DialError;
    use std::pin::Pin;

    #[derive(PartialEq)]
    enum ConnectionStatus {
        Disconnected,
        Dialling,
        Connected,
    }

    enum RegistrationStatus {
        RegisterOnNextConnection,
        Pending,
        Registered {
            re_register_in: Pin<Box<tokio::time::Sleep>>,
        },
    }

    pub struct Behaviour {
        inner: libp2p::rendezvous::client::Behaviour,
        rendezvous_point: Multiaddr,
        rendezvous_peer_id: PeerId,
        namespace: XmrBtcNamespace,
        registration_status: RegistrationStatus,
        connection_status: ConnectionStatus,
        registration_ttl: Option<u64>,
    }

    impl Behaviour {
        pub fn new(
            identity: identity::Keypair,
            rendezvous_peer_id: PeerId,
            rendezvous_address: Multiaddr,
            namespace: XmrBtcNamespace,
            registration_ttl: Option<u64>,
        ) -> Self {
            Self {
                inner: libp2p::rendezvous::client::Behaviour::new(identity),
                rendezvous_point: rendezvous_address,
                rendezvous_peer_id,
                namespace,
                registration_status: RegistrationStatus::RegisterOnNextConnection,
                connection_status: ConnectionStatus::Disconnected,
                registration_ttl,
            }
        }

        fn register(&mut self) {
            self.inner.register(
                self.namespace.into(),
                self.rendezvous_peer_id,
                self.registration_ttl,
            );
        }
    }

    impl NetworkBehaviour for Behaviour {
        type ProtocolsHandler =
            <libp2p::rendezvous::client::Behaviour as NetworkBehaviour>::ProtocolsHandler;
        type OutEvent = libp2p::rendezvous::client::Event;

        fn new_handler(&mut self) -> Self::ProtocolsHandler {
            self.inner.new_handler()
        }

        fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
            if peer_id == &self.rendezvous_peer_id {
                return vec![self.rendezvous_point.clone()];
            }

            vec![]
        }

        fn inject_connected(&mut self, peer_id: &PeerId) {
            if peer_id == &self.rendezvous_peer_id {
                self.connection_status = ConnectionStatus::Connected;

                match &self.registration_status {
                    RegistrationStatus::RegisterOnNextConnection => {
                        self.register();
                        self.registration_status = RegistrationStatus::Pending;
                    }
                    RegistrationStatus::Registered { .. } => {}
                    RegistrationStatus::Pending => {}
                }
            }
        }

        fn inject_disconnected(&mut self, peer_id: &PeerId) {
            if peer_id == &self.rendezvous_peer_id {
                self.connection_status = ConnectionStatus::Disconnected;
            }
        }

        fn inject_event(
            &mut self,
            peer_id: PeerId,
            connection: ConnectionId,
            event: <<Self::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutEvent,
        ) {
            self.inner.inject_event(peer_id, connection, event)
        }

        fn inject_dial_failure(
            &mut self,
            peer_id: Option<PeerId>,
            _handler: Self::ProtocolsHandler,
            _error: &DialError,
        ) {
            if let Some(id) = peer_id {
                if id == self.rendezvous_peer_id {
                    self.connection_status = ConnectionStatus::Disconnected;
                }
            }
        }

        #[allow(clippy::type_complexity)]
        fn poll(
            &mut self,
            cx: &mut std::task::Context<'_>,
            params: &mut impl PollParameters,
        ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ProtocolsHandler>> {
            match &mut self.registration_status {
                RegistrationStatus::RegisterOnNextConnection => match self.connection_status {
                    ConnectionStatus::Disconnected => {
                        self.connection_status = ConnectionStatus::Dialling;

                        return Poll::Ready(NetworkBehaviourAction::Dial {
                            opts: DialOpts::peer_id(self.rendezvous_peer_id)
                                .condition(PeerCondition::Disconnected)
                                .build(),

                            handler: Self::ProtocolsHandler::new(Duration::from_secs(30)),
                        });
                    }
                    ConnectionStatus::Dialling => {}
                    ConnectionStatus::Connected => {
                        self.registration_status = RegistrationStatus::Pending;
                        self.register();
                    }
                },
                RegistrationStatus::Registered { re_register_in } => {
                    if let Poll::Ready(()) = re_register_in.poll_unpin(cx) {
                        match self.connection_status {
                            ConnectionStatus::Connected => {
                                self.registration_status = RegistrationStatus::Pending;
                                self.register();
                            }
                            ConnectionStatus::Disconnected => {
                                self.registration_status =
                                    RegistrationStatus::RegisterOnNextConnection;

                                return Poll::Ready(NetworkBehaviourAction::Dial {
                                    opts: DialOpts::peer_id(self.rendezvous_peer_id)
                                        .condition(PeerCondition::Disconnected)
                                        .build(),
                                    handler: Self::ProtocolsHandler::new(Duration::from_secs(30)),
                                });
                            }
                            ConnectionStatus::Dialling => {}
                        }
                    }
                }
                RegistrationStatus::Pending => {}
            }

            let inner_poll = self.inner.poll(cx, params);

            // reset the timer if we successfully registered
            if let Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                libp2p::rendezvous::client::Event::Registered { ttl, .. },
            )) = &inner_poll
            {
                let half_of_ttl = Duration::from_secs(*ttl) / 2;

                self.registration_status = RegistrationStatus::Registered {
                    re_register_in: Box::pin(tokio::time::sleep(half_of_ttl)),
                };
            }

            inner_poll
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::network::test::{new_swarm, SwarmExt};
        use futures::StreamExt;
        use libp2p::rendezvous;
        use libp2p::swarm::SwarmEvent;

        #[tokio::test]
        async fn given_no_initial_connection_when_constructed_asb_connects_and_registers_with_rendezvous_node(
        ) {
            let mut rendezvous_node = new_swarm(|_, _| {
                rendezvous::server::Behaviour::new(rendezvous::server::Config::default())
            });
            let rendezvous_address = rendezvous_node.listen_on_random_memory_address().await;

            let mut asb = new_swarm(|_, identity| {
                rendezous::Behaviour::new(
                    identity,
                    *rendezvous_node.local_peer_id(),
                    rendezvous_address,
                    XmrBtcNamespace::Testnet,
                    None,
                )
            });
            asb.listen_on_random_memory_address().await; // this adds an external address

            tokio::spawn(async move {
                loop {
                    rendezvous_node.next().await;
                }
            });
            let asb_registered = tokio::spawn(async move {
                loop {
                    if let SwarmEvent::Behaviour(rendezvous::client::Event::Registered { .. }) =
                        asb.select_next_some().await
                    {
                        break;
                    }
                }
            });

            tokio::time::timeout(Duration::from_secs(10), asb_registered)
                .await
                .unwrap()
                .unwrap();
        }

        #[tokio::test]
        async fn asb_automatically_re_registers() {
            let mut rendezvous_node = new_swarm(|_, _| {
                rendezvous::server::Behaviour::new(
                    rendezvous::server::Config::default().with_min_ttl(2),
                )
            });
            let rendezvous_address = rendezvous_node.listen_on_random_memory_address().await;

            let mut asb = new_swarm(|_, identity| {
                rendezous::Behaviour::new(
                    identity,
                    *rendezvous_node.local_peer_id(),
                    rendezvous_address,
                    XmrBtcNamespace::Testnet,
                    Some(5),
                )
            });
            asb.listen_on_random_memory_address().await; // this adds an external address

            tokio::spawn(async move {
                loop {
                    rendezvous_node.next().await;
                }
            });
            let asb_registered_three_times = tokio::spawn(async move {
                let mut number_of_registrations = 0;

                loop {
                    if let SwarmEvent::Behaviour(rendezvous::client::Event::Registered { .. }) =
                        asb.select_next_some().await
                    {
                        number_of_registrations += 1
                    }

                    if number_of_registrations == 3 {
                        break;
                    }
                }
            });

            tokio::time::timeout(Duration::from_secs(30), asb_registered_three_times)
                .await
                .unwrap()
                .unwrap();
        }
    }
}
