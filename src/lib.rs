use libp2p::swarm::{ProtocolsHandler, ProtocolsHandlerEvent, ProtocolsHandlerUpgrErr, KeepAlive, SubstreamProtocol, NegotiatedSubstream, NetworkBehaviour, NetworkBehaviourAction, PollParameters, NotifyHandler};
use libp2p::futures::task::{Context, Poll};
use libp2p::{InboundUpgrade, PeerId, OutboundUpgrade};
use libp2p::core::{UpgradeInfo, Multiaddr};
use libp2p::futures::{FutureExt};
use std::future::{Ready, Future};
use std::convert::Infallible;
use libp2p::futures::future::BoxFuture;
use std::collections::VecDeque;
use libp2p::swarm::protocols_handler::OutboundUpgradeSend;
use libp2p::core::connection::ConnectionId;
use std::iter;
use std::task::Waker;

#[cfg(test)]
mod swarm_harness;

pub struct NMessageHandler<TInboundOut, TOutboundOut, TErr> {
    inbound_substream: Option<NegotiatedSubstream>,
    outbound_substream: Option<NegotiatedSubstream>,

    inbound_future: Option<BoxFuture<'static, Result<TInboundOut, TErr>>>,
    inbound_future_fn: Option<Box<dyn FnOnce(NegotiatedSubstream) -> BoxFuture<'static, Result<TInboundOut, TErr>> + Send + 'static>>,

    // todo: make enum for state
    outbound_future: Option<BoxFuture<'static, Result<TOutboundOut, TErr>>>,
    outbound_future_fn: Option<Box<dyn FnOnce(NegotiatedSubstream) -> BoxFuture<'static, Result<TOutboundOut, TErr>> + Send + 'static>>,

    substream_request: Option<SubstreamProtocol<NMessageProtocol, ()>>,

    info: &'static [u8]
}

impl<TInboundOut, TOutboundOut, TErr> NMessageHandler<TInboundOut, TOutboundOut, TErr> {
    pub fn new(info: &'static [u8]) -> Self {
        Self {
            inbound_substream: None,
            inbound_future: None,
            outbound_substream: None,
            outbound_future: None,
            outbound_future_fn: None,
            substream_request: None,
            info,
            inbound_future_fn: None
        }
    }
}

pub struct NMessageProtocol {
    info: &'static [u8]
}

impl NMessageProtocol {
    fn new(info: &'static [u8]) -> Self {
        Self {
            info
        }
    }
}

impl UpgradeInfo for NMessageProtocol {
    type Info = &'static [u8];
    type InfoIter = iter::Once<&'static [u8]>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(self.info)
    }
}

impl InboundUpgrade<NegotiatedSubstream> for NMessageProtocol {
    type Output = NegotiatedSubstream;
    type Error = Infallible;
    type Future = Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, socket: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        std::future::ready(Ok(socket))
    }
}

impl OutboundUpgrade<NegotiatedSubstream> for NMessageProtocol {
    type Output = NegotiatedSubstream;
    type Error = Infallible;
    type Future = Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, socket: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        std::future::ready(Ok(socket))
    }
}

pub enum ProtocolInEvent<I, O, E> {
    ExecuteInbound(Box<dyn FnOnce(NegotiatedSubstream) -> BoxFuture<'static, Result<I, E>> + Send + 'static>),
    ExecuteOutbound(Box<dyn FnOnce(NegotiatedSubstream) -> BoxFuture<'static, Result<O, E>> + Send + 'static>),
}

pub enum ProtocolOutEvent<I, O, E> {
    InboundFinished(I),
    OutboundFinished(O),
    InboundFailed(E),
    OutboundFailed(E),
}

impl<TInboundOut, TOutboundOut, TErr> ProtocolsHandler for NMessageHandler<TInboundOut, TOutboundOut, TErr> where TInboundOut: Send + 'static, TOutboundOut: Send + 'static, TErr: Send + 'static {
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

    fn inject_fully_negotiated_inbound(&mut self, protocol: NegotiatedSubstream, _: Self::InboundOpenInfo) {
        log::info!("inject_fully_negotiated_inbound");

        if let Some(future_fn) = self.inbound_future_fn.take() {
            self.inbound_future = Some(future_fn(protocol))
        } else {
            self.inbound_substream = Some(protocol)
        }
    }

    fn inject_fully_negotiated_outbound(&mut self, protocol: NegotiatedSubstream, _: Self::OutboundOpenInfo) {
        log::info!("inject_fully_negotiated_outbound");

        if let Some(future_fn) = self.outbound_future_fn.take() {
            self.outbound_future = Some(future_fn(protocol))
        } else {
            self.outbound_substream = Some(protocol)
        }
    }

    fn inject_event(&mut self, event: Self::InEvent) {
        match event {
            ProtocolInEvent::ExecuteInbound(protocol_fn) => {
                log::trace!("got execute inbound event");

                match self.inbound_substream.take() {
                    Some(substream) => {
                        log::trace!("got inbound substream, upgrading with custom protocol");

                        self.inbound_future = Some(protocol_fn(substream))
                    }
                    None => {
                        self.inbound_future_fn = Some(protocol_fn);
                    }
                }
            }
            ProtocolInEvent::ExecuteOutbound(protocol_fn) => {
                log::trace!("got execute outbound event");

                self.substream_request = Some(SubstreamProtocol::new(NMessageProtocol::new(self.info), ()));

                match self.outbound_substream.take() {
                    Some(substream) => {
                        log::trace!("got outbound substream, upgrading with custom protocol");

                        self.outbound_future = Some(protocol_fn(substream));
                    }
                    None => {
                        self.outbound_future_fn = Some(protocol_fn);
                    }
                }
            }
        }
    }

    fn inject_dial_upgrade_error(&mut self, _: Self::OutboundOpenInfo, _: ProtocolsHandlerUpgrErr<
        <Self::OutboundProtocol as OutboundUpgradeSend>::Error
    >
    ) {
        unimplemented!("TODO: handle this")
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        KeepAlive::Yes
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent, Self::Error>> {
        if let Some(protocol) = self.substream_request.take() {
            return Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest { protocol })
        }

        if let Some(future) = self.inbound_future.as_mut() {
            match future.poll_unpin(cx) {
                Poll::Ready(Ok(value)) => {
                    return Poll::Ready(ProtocolsHandlerEvent::Custom(ProtocolOutEvent::InboundFinished(value)))
                }
                Poll::Ready(Err(e)) => {
                    return Poll::Ready(ProtocolsHandlerEvent::Custom(ProtocolOutEvent::InboundFailed(e)))
                }
                Poll::Pending => {}
            }
        }

        if let Some(future) = self.outbound_future.as_mut() {
            match future.poll_unpin(cx) {
                Poll::Ready(Ok(value)) => {
                    return Poll::Ready(ProtocolsHandlerEvent::Custom(ProtocolOutEvent::OutboundFinished(value)))
                }
                Poll::Ready(Err(e)) => {
                    return Poll::Ready(ProtocolsHandlerEvent::Custom(ProtocolOutEvent::OutboundFailed(e)))
                }
                Poll::Pending => {}
            }
        }

        Poll::Pending
    }
}

pub struct NMessageBehaviour<I, O, E> {
    protocol_in_events: VecDeque<(PeerId, ProtocolInEvent<I, O, E>)>,
    protocol_out_events: VecDeque<(PeerId, ProtocolOutEvent<I, O, E>)>,

    waker: Option<Waker>,

    connected_peers: Vec<PeerId>,

    info: &'static [u8]
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
            protocol_in_events: Default::default(),
            protocol_out_events: Default::default(),
            waker: None,
            connected_peers: vec![],
            info
        }
    }
}

impl<I, O, E> NMessageBehaviour<I, O, E> {
    pub fn do_protocol_listener<F>(&mut self, peer: PeerId, protocol: impl FnOnce(NegotiatedSubstream) -> F + Send + 'static ) where F: Future<Output = Result<I, E>> + Send + 'static {
        self.protocol_in_events.push_back((peer, ProtocolInEvent::ExecuteInbound(Box::new(move |substream| protocol(substream).boxed()))));

        log::info!("pushing ExecuteInbound event");

        if let Some(waker) = self.waker.take() {
            log::trace!("waking task");

            waker.wake();
        }
    }

    pub fn do_protocol_dialer<F>(&mut self, peer: PeerId, protocol: impl FnOnce(NegotiatedSubstream) -> F + Send + 'static ) where F: Future<Output = Result<O, E>> + Send + 'static  {
        self.protocol_in_events.push_back((peer, ProtocolInEvent::ExecuteOutbound(Box::new(move |substream| protocol(substream).boxed()))));

        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

#[derive(Clone)]
pub enum BehaviourOutEvent<I, O, E> {
    InboundFinished(PeerId, I),
    OutboundFinished(PeerId, O),
    InboundFailed(PeerId, E),
    OutboundFailed(PeerId, E),
}

impl<I, O, E> NetworkBehaviour for NMessageBehaviour<I, O, E> where I: Send + 'static, O: Send + 'static, E: Send + 'static  {
    type ProtocolsHandler = NMessageHandler<I, O, E>;
    type OutEvent = BehaviourOutEvent<I, O, E>;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        NMessageHandler::new(self.info)
    }

    fn addresses_of_peer(&mut self, _: &PeerId) -> Vec<Multiaddr> {
        Vec::new()
    }

    fn inject_connected(&mut self, peer: &PeerId) {
        self.connected_peers.push(peer.clone());
    }

    fn inject_disconnected(&mut self, peer: &PeerId) {
        self.connected_peers.retain(|p| p != peer)
    }

    fn inject_event(&mut self, peer: PeerId, _: ConnectionId, event: ProtocolOutEvent<I, O, E>) {
        self.protocol_out_events.push_back((peer, event));
    }

    fn poll(&mut self, cx: &mut Context<'_>, params: &mut impl PollParameters) -> Poll<NetworkBehaviourAction<ProtocolInEvent<I, O, E>, Self::OutEvent>> {
        log::debug!("peer {}, no. events {}", params.local_peer_id(), self.protocol_in_events.len());

        if let Some((peer, event)) = self.protocol_in_events.pop_front() {
            log::debug!("notifying handler");

            if !self.connected_peers.contains(&peer) {
                log::info!("not connected to peer {}, waiting ...", peer);
                self.protocol_in_events.push_back((peer, event));
            } else {
                return Poll::Ready(NetworkBehaviourAction::NotifyHandler { peer_id: peer, handler: NotifyHandler::Any, event})
            }
        }

        if let Some((peer, event)) = self.protocol_out_events.pop_front() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(match event {
                ProtocolOutEvent::InboundFinished(event) => BehaviourOutEvent::InboundFinished(peer, event),
                ProtocolOutEvent::OutboundFinished(event) => BehaviourOutEvent::OutboundFinished(peer, event),
                ProtocolOutEvent::InboundFailed(e) => BehaviourOutEvent::InboundFailed(peer, e),
                ProtocolOutEvent::OutboundFailed(e) => BehaviourOutEvent::OutboundFailed(peer, e)
            }))
        }

        self.waker = Some(cx.waker().clone());

        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::core::upgrade;
    use anyhow::{Context, Error};
    use swarm_harness::new_connected_swarm_pair;
    use libp2p::swarm::SwarmEvent;
    use libp2p::futures::future::join;

    #[derive(serde::Serialize, serde::Deserialize)]
    #[derive(Debug)]
    struct Message0 {
        foo: u32
    }
    #[derive(serde::Serialize, serde::Deserialize)]
    #[derive(Debug)]
    struct Message1 {
        bar: u32
    }
    #[derive(serde::Serialize, serde::Deserialize)]
    #[derive(Debug)]
    struct Message2 {
        baz: u32
    }

    #[derive(Debug)]
    struct AliceResult {
        bar: u32
    }
    #[derive(Debug)]
    struct BobResult {
        foo: u32,
        baz: u32
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
                BehaviourOutEvent::InboundFailed(_, e) | BehaviourOutEvent::OutboundFailed(_, e)  => MyOutEvent::Failed(e)
            }
        }
    }

    #[derive(libp2p::NetworkBehaviour)]
    #[behaviour(out_event = "MyOutEvent", event_process = false)]
    struct MyBehaviour {
        inner: NMessageBehaviour<BobResult, AliceResult, anyhow::Error>
    }

    impl MyBehaviour {
        pub fn new() -> Self {
            Self {
                inner: NMessageBehaviour::new(b"/foo/bar/1.0.0")
            }
        }
    }

    impl MyBehaviour {
        fn alice_do_protocol(&mut self, bob: PeerId, foo: u32, baz: u32) {
            self.inner.do_protocol_dialer(bob, move |mut substream| async move {
                log::trace!("alice starting protocol");

                upgrade::write_one(&mut substream, serde_cbor::to_vec(&Message0 {
                    foo
                }).context("failed to serialize Message0")?).await?;

                log::trace!("alice sent message0");

                let bytes = upgrade::read_one(&mut substream, 1024).await?;
                let message1 = serde_cbor::from_slice::<Message1>(&bytes)?;

                log::trace!("alice read message1");

                upgrade::write_one(&mut substream, serde_cbor::to_vec(&Message2 {
                    baz
                }).context("failed to serialize Message2")?).await?;

                log::trace!("alice sent message2");

                log::trace!("alice finished");

                Ok(AliceResult {
                    bar: message1.bar
                })
            })
        }

        fn bob_do_protocol(&mut self, alice: PeerId, bar: u32) {
            self.inner.do_protocol_listener(alice, move |mut substream| async move {
                log::trace!("bob start protocol");

                let bytes = upgrade::read_one(&mut substream, 1024).await?;
                let message0 = serde_cbor::from_slice::<Message0>(&bytes)?;

                log::trace!("bob read message0");

                upgrade::write_one(&mut substream, serde_cbor::to_vec(&Message1 {
                    bar
                }).context("failed to serialize Message1")?).await?;

                log::trace!("bob sent message1");

                let bytes = upgrade::read_one(&mut substream, 1024).await?;
                let message2 = serde_cbor::from_slice::<Message2>(&bytes)?;

                log::trace!("bob read message2");

                log::trace!("bob finished");

                Ok(BobResult {
                    foo: message0.foo,
                    baz: message2.baz
                })
            })
        }
    }

    #[tokio::test]
    async fn it_works() {
        let _ = env_logger::try_init();

        let (mut alice, mut bob) = new_connected_swarm_pair(|_, _| MyBehaviour::new()).await;

        log::info!("alice = {}", alice.peer_id);
        log::info!("bob = {}", bob.peer_id);

        alice.swarm.alice_do_protocol(bob.peer_id, 10, 42);
        bob.swarm.bob_do_protocol(alice.peer_id, 1337);

        let alice_handle = tokio::spawn(async move { alice.swarm.next_event().await });
        let bob_handle = tokio::spawn(async move { bob.swarm.next_event().await });

        let (alice_event, bob_event) = join(alice_handle, bob_handle).await;

        assert!(matches!(dbg!(alice_event.unwrap()), SwarmEvent::Behaviour(MyOutEvent::Alice(AliceResult {
            bar: 1337
        }))));
        assert!(matches!(dbg!(bob_event.unwrap()), SwarmEvent::Behaviour(MyOutEvent::Bob(BobResult {
            foo: 10,
            baz: 42
        }))));
    }
}
