use libp2p::core::connection::ConnectionId;
use libp2p::core::{ConnectedPoint, Multiaddr, UpgradeInfo};
use libp2p::futures::future::BoxFuture;
use libp2p::futures::task::{Context, Poll};
use libp2p::futures::FutureExt;
use libp2p::swarm::protocols_handler::OutboundUpgradeSend;
use libp2p::swarm::{
    KeepAlive, NegotiatedSubstream, NetworkBehaviour, NetworkBehaviourAction, NotifyHandler,
    PollParameters, ProtocolsHandler, ProtocolsHandlerEvent, ProtocolsHandlerUpgrErr,
    SubstreamProtocol,
};
use libp2p::{InboundUpgrade, OutboundUpgrade, PeerId};
use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use std::future::{Future, Ready};
use std::{iter, mem};

#[cfg(test)]
mod swarm_harness;

type Protocol<T, E> = BoxFuture<'static, Result<T, E>>;
type InboundProtocolFn<I, E> = Box<dyn FnOnce(InboundSubstream) -> Protocol<I, E> + Send + 'static>;
type OutboundProtocolFn<O, E> =
    Box<dyn FnOnce(OutboundSubstream) -> Protocol<O, E> + Send + 'static>;

enum InboundProtocolState<T, E> {
    GotFunctionNeedSubstream(InboundProtocolFn<T, E>),
    GotSubstreamNeedFunction(InboundSubstream),
    Executing(Protocol<T, E>),
}

enum OutboundProtocolState<T, E> {
    GotFunctionNeedSubstream(OutboundProtocolFn<T, E>),
    GotFunctionRequestedSubstream(OutboundProtocolFn<T, E>),
    Executing(Protocol<T, E>),
}

enum ProtocolState<I, O, E> {
    None,
    Inbound(InboundProtocolState<I, E>),
    Outbound(OutboundProtocolState<O, E>),
    Done,
    Poisoned,
}

pub struct NMessageHandler<TInboundOut, TOutboundOut, TErr> {
    state: ProtocolState<TInboundOut, TOutboundOut, TErr>,
    info: &'static [u8],
}

impl<TInboundOut, TOutboundOut, TErr> NMessageHandler<TInboundOut, TOutboundOut, TErr> {
    pub fn new(info: &'static [u8]) -> Self {
        Self {
            state: ProtocolState::None,
            info,
        }
    }
}

pub struct NMessageProtocol {
    info: &'static [u8],
}

impl NMessageProtocol {
    fn new(info: &'static [u8]) -> Self {
        Self { info }
    }
}

impl UpgradeInfo for NMessageProtocol {
    type Info = &'static [u8];
    type InfoIter = iter::Once<&'static [u8]>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(self.info)
    }
}

pub struct InboundSubstream(NegotiatedSubstream);

pub struct OutboundSubstream(NegotiatedSubstream);

impl InboundUpgrade<NegotiatedSubstream> for NMessageProtocol {
    type Output = InboundSubstream;
    type Error = Infallible;
    type Future = Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, socket: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        std::future::ready(Ok(InboundSubstream(socket)))
    }
}

impl OutboundUpgrade<NegotiatedSubstream> for NMessageProtocol {
    type Output = OutboundSubstream;
    type Error = Infallible;
    type Future = Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, socket: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        std::future::ready(Ok(OutboundSubstream(socket)))
    }
}

pub enum ProtocolInEvent<I, O, E> {
    ExecuteInbound(InboundProtocolFn<I, E>),
    ExecuteOutbound(OutboundProtocolFn<O, E>),
}

// TODO: Remove Finished/Failed and just wrap a Result
pub enum ProtocolOutEvent<I, O, E> {
    InboundFinished(I),
    OutboundFinished(O),
    InboundFailed(E),
    OutboundFailed(E),
}

impl<TInboundOut, TOutboundOut, TErr> ProtocolsHandler
    for NMessageHandler<TInboundOut, TOutboundOut, TErr>
where
    TInboundOut: Send + 'static,
    TOutboundOut: Send + 'static,
    TErr: Send + 'static,
{
    type InEvent = ProtocolInEvent<TInboundOut, TOutboundOut, TErr>;
    type OutEvent = ProtocolOutEvent<TInboundOut, TOutboundOut, TErr>;
    type Error = std::io::Error;
    type InboundProtocol = NMessageProtocol;
    type OutboundProtocol = NMessageProtocol;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(NMessageProtocol::new(self.info), ())
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        substream: InboundSubstream,
        _: Self::InboundOpenInfo,
    ) {
        match mem::replace(&mut self.state, ProtocolState::Poisoned) {
            ProtocolState::None => {
                self.state = ProtocolState::Inbound(
                    InboundProtocolState::GotSubstreamNeedFunction(substream),
                );
            }
            ProtocolState::Inbound(InboundProtocolState::GotFunctionNeedSubstream(protocol_fn)) => {
                self.state =
                    ProtocolState::Inbound(InboundProtocolState::Executing(protocol_fn(substream)));
            }
            ProtocolState::Inbound(_) | ProtocolState::Done => {
                panic!("Illegal state, substream is already present.");
            }
            ProtocolState::Outbound(_) => {
                panic!("Failed to process inbound substream in outbound protocol.");
            }
            ProtocolState::Poisoned => {
                panic!("Illegal state, currently in transient state poisoned.");
            }
        }
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        substream: OutboundSubstream,
        _: Self::OutboundOpenInfo,
    ) {
        match mem::replace(&mut self.state, ProtocolState::Poisoned) {
            ProtocolState::Outbound(OutboundProtocolState::GotFunctionRequestedSubstream(
                protocol_fn,
            )) => {
                self.state = ProtocolState::Outbound(OutboundProtocolState::Executing(
                    protocol_fn(substream),
                ));
            }
            ProtocolState::None
            | ProtocolState::Outbound(OutboundProtocolState::GotFunctionNeedSubstream(_)) => {
                panic!("Illegal state, receiving substream means it was requested.");
            }
            ProtocolState::Outbound(_) | ProtocolState::Done => {
                panic!("Illegal state, substream is already present.");
            }
            ProtocolState::Inbound(_) => {
                panic!("Failed to process outbound substream in inbound protocol.");
            }
            ProtocolState::Poisoned => {
                panic!("Illegal state, currently in transient state poisoned.");
            }
        }
    }

    fn inject_event(&mut self, event: Self::InEvent) {
        match event {
            ProtocolInEvent::ExecuteInbound(protocol_fn) => {
                match mem::replace(&mut self.state, ProtocolState::Poisoned) {
                    ProtocolState::None => {
                        self.state = ProtocolState::Inbound(
                            InboundProtocolState::GotFunctionNeedSubstream(protocol_fn),
                        );
                    }
                    ProtocolState::Inbound(InboundProtocolState::GotSubstreamNeedFunction(
                        substream,
                    )) => {
                        self.state = ProtocolState::Inbound(InboundProtocolState::Executing(
                            protocol_fn(substream),
                        ));
                    }
                    ProtocolState::Inbound(_) | ProtocolState::Done => {
                        panic!("Illegal state, protocol fn is already present.");
                    }
                    ProtocolState::Outbound(_) => {
                        panic!("Failed to process inbound protocol fn in outbound protocol.");
                    }
                    ProtocolState::Poisoned => {
                        panic!("Illegal state, currently in transient state poisoned.");
                    }
                }
            }
            ProtocolInEvent::ExecuteOutbound(protocol_fn) => {
                match mem::replace(&mut self.state, ProtocolState::Poisoned) {
                    ProtocolState::None => {
                        self.state = ProtocolState::Outbound(
                            OutboundProtocolState::GotFunctionNeedSubstream(protocol_fn),
                        );
                    }
                    ProtocolState::Outbound(_) | ProtocolState::Done => {
                        panic!("Illegal state, protocol fn is already present.");
                    }
                    ProtocolState::Inbound(_) => {
                        panic!("Failed to process outbound protocol fn in inbound protocol.");
                    }
                    ProtocolState::Poisoned => {
                        panic!("Illegal state, currently in transient state poisoned.");
                    }
                }
            }
        }
    }

    fn inject_dial_upgrade_error(
        &mut self,
        _: Self::OutboundOpenInfo,
        _: ProtocolsHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgradeSend>::Error>,
    ) {
        unimplemented!("TODO: handle this")
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        KeepAlive::Yes
    }

    #[allow(clippy::type_complexity)]
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ProtocolsHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::OutEvent,
            Self::Error,
        >,
    > {
        match mem::replace(&mut self.state, ProtocolState::Poisoned) {
            ProtocolState::Inbound(InboundProtocolState::Executing(mut protocol)) => match protocol
                .poll_unpin(cx)
            {
                Poll::Ready(Ok(value)) => {
                    self.state = ProtocolState::Done;
                    Poll::Ready(ProtocolsHandlerEvent::Custom(
                        ProtocolOutEvent::InboundFinished(value),
                    ))
                }
                Poll::Ready(Err(e)) => {
                    self.state = ProtocolState::Done;
                    Poll::Ready(ProtocolsHandlerEvent::Custom(
                        ProtocolOutEvent::InboundFailed(e),
                    ))
                }
                Poll::Pending => {
                    self.state = ProtocolState::Inbound(InboundProtocolState::Executing(protocol));
                    Poll::Pending
                }
            },
            ProtocolState::Outbound(OutboundProtocolState::Executing(mut protocol)) => {
                match protocol.poll_unpin(cx) {
                    Poll::Ready(Ok(value)) => {
                        self.state = ProtocolState::Done;
                        Poll::Ready(ProtocolsHandlerEvent::Custom(
                            ProtocolOutEvent::OutboundFinished(value),
                        ))
                    }
                    Poll::Ready(Err(e)) => {
                        self.state = ProtocolState::Done;
                        Poll::Ready(ProtocolsHandlerEvent::Custom(
                            ProtocolOutEvent::OutboundFailed(e),
                        ))
                    }
                    Poll::Pending => {
                        self.state =
                            ProtocolState::Outbound(OutboundProtocolState::Executing(protocol));
                        Poll::Pending
                    }
                }
            }
            ProtocolState::Outbound(OutboundProtocolState::GotFunctionNeedSubstream(protocol)) => {
                self.state = ProtocolState::Outbound(
                    OutboundProtocolState::GotFunctionRequestedSubstream(protocol),
                );
                Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    protocol: SubstreamProtocol::new(NMessageProtocol::new(self.info), ()),
                })
            }
            ProtocolState::Poisoned => {
                unreachable!("Protocol is poisoned (transient state)")
            }
            other => {
                self.state = other;
                Poll::Pending
            }
        }
    }
}

pub struct NMessageBehaviour<I, O, E> {
    protocol_in_events: VecDeque<(PeerId, ProtocolInEvent<I, O, E>)>,
    protocol_out_events: VecDeque<(PeerId, ProtocolOutEvent<I, O, E>)>,

    connected_peers: HashMap<PeerId, Vec<Multiaddr>>,

    info: &'static [u8],
}

impl<I, O, E> NMessageBehaviour<I, O, E> {
    /// Constructs a new [`NMessageBehaviour`] with the given protocol info.
    ///
    /// # Example
    ///
    /// ```
    /// # use libp2p_nmessage::NMessageBehaviour;
    ///
    /// let _ = NMessageBehaviour::new(b"/foo/bar/1.0.0");
    /// ```
    pub fn new(info: &'static [u8]) -> Self {
        Self {
            protocol_in_events: VecDeque::default(),
            protocol_out_events: VecDeque::default(),
            connected_peers: HashMap::default(),
            info,
        }
    }
}

impl<I, O, E> NMessageBehaviour<I, O, E> {
    pub fn do_protocol_listener<F>(
        &mut self,
        peer: PeerId,
        protocol: impl FnOnce(InboundSubstream) -> F + Send + 'static,
    ) where
        F: Future<Output = Result<I, E>> + Send + 'static,
    {
        self.protocol_in_events.push_back((
            peer,
            ProtocolInEvent::ExecuteInbound(Box::new(move |substream| protocol(substream).boxed())),
        ));
    }

    pub fn do_protocol_dialer<F>(
        &mut self,
        peer: PeerId,
        protocol: impl FnOnce(OutboundSubstream) -> F + Send + 'static,
    ) where
        F: Future<Output = Result<O, E>> + Send + 'static,
    {
        self.protocol_in_events.push_back((
            peer,
            ProtocolInEvent::ExecuteOutbound(Box::new(move |substream| {
                protocol(substream).boxed()
            })),
        ));
    }
}

#[derive(Clone)]
pub enum BehaviourOutEvent<I, O, E> {
    InboundFinished(PeerId, I),
    OutboundFinished(PeerId, O),
    InboundFailed(PeerId, E),
    OutboundFailed(PeerId, E),
}

impl<I, O, E> NetworkBehaviour for NMessageBehaviour<I, O, E>
where
    I: Send + 'static,
    O: Send + 'static,
    E: Send + 'static,
{
    type ProtocolsHandler = NMessageHandler<I, O, E>;
    type OutEvent = BehaviourOutEvent<I, O, E>;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        NMessageHandler::new(self.info)
    }

    fn addresses_of_peer(&mut self, peer: &PeerId) -> Vec<Multiaddr> {
        self.connected_peers.get(peer).cloned().unwrap_or_default()
    }

    fn inject_connected(&mut self, _: &PeerId) {}

    fn inject_disconnected(&mut self, _: &PeerId) {}

    fn inject_connection_established(
        &mut self,
        peer: &PeerId,
        _: &ConnectionId,
        point: &ConnectedPoint,
    ) {
        let multiaddr = point.get_remote_address().clone();

        self.connected_peers
            .entry(*peer)
            .or_default()
            .push(multiaddr);
    }

    fn inject_connection_closed(
        &mut self,
        peer: &PeerId,
        _: &ConnectionId,
        point: &ConnectedPoint,
    ) {
        let multiaddr = point.get_remote_address();

        self.connected_peers
            .entry(*peer)
            .or_default()
            .retain(|addr| addr != multiaddr);
    }

    fn inject_event(&mut self, peer: PeerId, _: ConnectionId, event: ProtocolOutEvent<I, O, E>) {
        self.protocol_out_events.push_back((peer, event));
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<ProtocolInEvent<I, O, E>, Self::OutEvent>> {
        if let Some((peer, event)) = self.protocol_in_events.pop_front() {
            if !self.connected_peers.contains_key(&peer) {
                self.protocol_in_events.push_back((peer, event));
            } else {
                return Poll::Ready(NetworkBehaviourAction::NotifyHandler {
                    peer_id: peer,
                    handler: NotifyHandler::Any,
                    event,
                });
            }
        }

        if let Some((peer, event)) = self.protocol_out_events.pop_front() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(match event {
                ProtocolOutEvent::InboundFinished(event) => {
                    BehaviourOutEvent::InboundFinished(peer, event)
                }
                ProtocolOutEvent::OutboundFinished(event) => {
                    BehaviourOutEvent::OutboundFinished(peer, event)
                }
                ProtocolOutEvent::InboundFailed(e) => BehaviourOutEvent::InboundFailed(peer, e),
                ProtocolOutEvent::OutboundFailed(e) => BehaviourOutEvent::OutboundFailed(peer, e),
            }));
        }

        Poll::Pending
    }
}

#[cfg(test)]
#[allow(clippy::blacklisted_name)]
mod tests {
    use super::*;
    use crate::swarm_harness::await_events_or_timeout;
    use anyhow::{Context, Error};
    use libp2p::core::upgrade;
    use libp2p::swarm::SwarmEvent;
    use swarm_harness::new_connected_swarm_pair;
    use tokio::runtime::Handle;

    #[derive(serde::Serialize, serde::Deserialize, Debug)]
    struct Message0 {
        foo: u32,
    }
    #[derive(serde::Serialize, serde::Deserialize, Debug)]
    struct Message1 {
        bar: u32,
    }
    #[derive(serde::Serialize, serde::Deserialize, Debug)]
    struct Message2 {
        baz: u32,
    }

    #[derive(Debug)]
    struct AliceResult {
        bar: u32,
    }
    #[derive(Debug)]
    struct BobResult {
        foo: u32,
        baz: u32,
    }

    #[derive(Debug)]
    enum MyOutEvent {
        Alice(AliceResult),
        Bob(BobResult),
        Failed(anyhow::Error),
    }

    impl From<BehaviourOutEvent<BobResult, AliceResult, anyhow::Error>> for MyOutEvent {
        fn from(event: BehaviourOutEvent<BobResult, AliceResult, Error>) -> Self {
            match event {
                BehaviourOutEvent::InboundFinished(_, bob) => MyOutEvent::Bob(bob),
                BehaviourOutEvent::OutboundFinished(_, alice) => MyOutEvent::Alice(alice),
                BehaviourOutEvent::InboundFailed(_, e)
                | BehaviourOutEvent::OutboundFailed(_, e) => MyOutEvent::Failed(e),
            }
        }
    }

    #[derive(libp2p::NetworkBehaviour)]
    #[behaviour(out_event = "MyOutEvent", event_process = false)]
    struct MyBehaviour {
        inner: NMessageBehaviour<BobResult, AliceResult, anyhow::Error>,
    }

    impl MyBehaviour {
        pub fn new() -> Self {
            Self {
                inner: NMessageBehaviour::new(b"/foo/bar/1.0.0"),
            }
        }
    }

    impl MyBehaviour {
        fn alice_do_protocol(&mut self, bob: PeerId, foo: u32, baz: u32) {
            self.inner
                .do_protocol_dialer(bob, move |mut substream| async move {
                    upgrade::write_with_len_prefix(
                        &mut substream.0,
                        serde_cbor::to_vec(&Message0 { foo })
                            .context("failed to serialize Message0")?,
                    )
                    .await?;

                    let bytes = upgrade::read_one(&mut substream.0, 1024).await?;
                    let message1 = serde_cbor::from_slice::<Message1>(&bytes)?;

                    upgrade::write_with_len_prefix(
                        &mut substream.0,
                        serde_cbor::to_vec(&Message2 { baz })
                            .context("failed to serialize Message2")?,
                    )
                    .await?;

                    Ok(AliceResult { bar: message1.bar })
                })
        }

        fn bob_do_protocol(&mut self, alice: PeerId, bar: u32) {
            self.inner
                .do_protocol_listener(alice, move |mut substream| async move {
                    let bytes = upgrade::read_one(&mut substream.0, 1024).await?;
                    let message0 = serde_cbor::from_slice::<Message0>(&bytes)?;

                    upgrade::write_with_len_prefix(
                        &mut substream.0,
                        serde_cbor::to_vec(&Message1 { bar })
                            .context("failed to serialize Message1")?,
                    )
                    .await?;

                    let bytes = upgrade::read_one(&mut substream.0, 1024).await?;
                    let message2 = serde_cbor::from_slice::<Message2>(&bytes)?;

                    Ok(BobResult {
                        foo: message0.foo,
                        baz: message2.baz,
                    })
                })
        }
    }

    #[tokio::test]
    async fn it_works() {
        let _ = env_logger::try_init();

        let (mut alice, mut bob) =
            new_connected_swarm_pair(|_, _| MyBehaviour::new(), Handle::current()).await;

        alice.swarm.alice_do_protocol(bob.peer_id, 10, 42);
        bob.swarm.bob_do_protocol(alice.peer_id, 1337);

        let (alice_event, bob_event) =
            await_events_or_timeout(alice.swarm.next_event(), bob.swarm.next_event()).await;

        assert!(matches!(
            alice_event,
            SwarmEvent::Behaviour(MyOutEvent::Alice(AliceResult { bar: 1337 }))
        ));
        assert!(matches!(
            bob_event,
            SwarmEvent::Behaviour(MyOutEvent::Bob(BobResult { foo: 10, baz: 42 }))
        ));
    }
}
