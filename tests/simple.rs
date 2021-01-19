#![allow(clippy::blacklisted_name)]

use anyhow::{Context, Error};
use harness::await_events_or_timeout;
use harness::new_connected_swarm_pair;
use libp2p::swarm::SwarmEvent;
use libp2p::PeerId;
use libp2p_nmessage::{BehaviourOutEvent, NMessageBehaviour};
use tokio::runtime::Handle;

mod harness;

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
            BehaviourOutEvent::Inbound(_, Ok(bob)) => MyOutEvent::Bob(bob),
            BehaviourOutEvent::Outbound(_, Ok(alice)) => MyOutEvent::Alice(alice),
            BehaviourOutEvent::Inbound(_, Err(e)) | BehaviourOutEvent::Outbound(_, Err(e)) => {
                MyOutEvent::Failed(e)
            }
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
                substream
                    .write_message(
                        &serde_cbor::to_vec(&Message0 { foo })
                            .context("failed to serialize Message0")?,
                    )
                    .await?;

                let bytes = substream.read_message(1024).await?;

                let message1 = serde_cbor::from_slice::<Message1>(&bytes)?;

                substream
                    .write_message(
                        &serde_cbor::to_vec(&Message2 { baz })
                            .context("failed to serialize Message2")?,
                    )
                    .await?;

                Ok(AliceResult { bar: message1.bar })
            })
    }

    fn bob_do_protocol(&mut self, alice: PeerId, bar: u32) {
        self.inner
            .do_protocol_listener(alice, move |mut substream| async move {
                let bytes = substream.read_message(1024).await?;
                let message0 = serde_cbor::from_slice::<Message0>(&bytes)?;

                substream
                    .write_message(
                        &serde_cbor::to_vec(&Message1 { bar })
                            .context("failed to serialize Message1")?,
                    )
                    .await?;

                let bytes = substream.read_message(1024).await?;
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
