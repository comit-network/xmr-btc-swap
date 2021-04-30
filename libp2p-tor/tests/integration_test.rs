use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport;
use libp2p::core::upgrade::Version;
use libp2p::ping::{Ping, PingEvent};
use libp2p::swarm::{SwarmBuilder, SwarmEvent};
use libp2p::tcp::tokio::TcpStream;
use libp2p::{noise, yamux, Swarm, Transport};
use libp2p_tor::torut_ext::AuthenticatedConnectionExt;
use libp2p_tor::{dial_only, duplex};
use rand::Rng;
use std::collections::HashMap;
use std::convert::Infallible;
use std::future::Future;
use std::time::Duration;
use testcontainers::{Container, Docker, Image, WaitForMessage};
use torut::control::AuthenticatedConn;

#[tokio::test(flavor = "multi_thread")]
async fn create_ephemeral_service() {
    tracing_subscriber::fmt().with_env_filter("debug").init();
    let wildcard_multiaddr =
        "/onion3/WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW:8080"
            .parse()
            .unwrap();

    // let docker = Cli::default();
    //
    // let tor1 = docker.run(TorImage::default().with_args(TorArgs {
    //     control_port: Some(9051),
    //     socks_port: None
    // }));
    // let tor2 = docker.run(TorImage::default());
    //
    // let tor1_control_port = tor1.get_host_port(9051).unwrap();
    // let tor2_socks_port = tor2.get_host_port(9050).unwrap();

    let mut listen_swarm = make_swarm(async move {
        let mut onion_key_bytes = [0u8; 64];
        rand::thread_rng().fill(&mut onion_key_bytes);

        duplex::TorConfig::new(
            AuthenticatedConn::with_password(9051, "supersecret")
                .await
                .unwrap(),
            move || onion_key_bytes.into(),
        )
        .await
        .unwrap()
        .boxed()
    })
    .await;
    let mut dial_swarm = make_swarm(async { dial_only::TorConfig::new(9050).boxed() }).await;

    listen_swarm.listen_on(wildcard_multiaddr).unwrap();

    let onion_listen_addr = loop {
        let event = listen_swarm.next_event().await;

        tracing::info!("{:?}", event);

        if let SwarmEvent::NewListenAddr(addr) = event {
            break addr;
        }
    };

    dial_swarm.dial_addr(onion_listen_addr).unwrap();

    loop {
        tokio::select! {
            event = listen_swarm.next_event() => {
                tracing::info!("{:?}", event);
            },
            event = dial_swarm.next_event() => {
                tracing::info!("{:?}", event);
            }
        }
    }
}

async fn make_swarm(
    transport_future: impl Future<Output = transport::Boxed<TcpStream>>,
) -> Swarm<Behaviour> {
    let identity = libp2p::identity::Keypair::generate_ed25519();

    let dh_keys = noise::Keypair::<noise::X25519Spec>::new()
        .into_authentic(&identity)
        .unwrap();
    let noise = noise::NoiseConfig::xx(dh_keys).into_authenticated();

    let transport = transport_future
        .await
        .upgrade(Version::V1)
        .authenticate(noise)
        .multiplex(yamux::YamuxConfig::default())
        .timeout(Duration::from_secs(20))
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    SwarmBuilder::new(
        transport,
        Behaviour::default(),
        identity.public().into_peer_id(),
    )
    .executor(Box::new(|f| {
        tokio::spawn(f);
    }))
    .build()
}

#[derive(Debug)]
enum OutEvent {
    Ping(PingEvent),
}

impl From<PingEvent> for OutEvent {
    fn from(e: PingEvent) -> Self {
        OutEvent::Ping(e)
    }
}

#[derive(libp2p::NetworkBehaviour, Default)]
#[behaviour(event_process = false, out_event = "OutEvent")]
struct Behaviour {
    ping: Ping,
}

#[derive(Default)]
struct TorImage {
    args: TorArgs,
}

impl TorImage {
    // fn control_port_password(&self) -> String {
    //     "supersecret".to_owned()
    // }
}

#[derive(Default, Copy, Clone)]
struct TorArgs {
    control_port: Option<u16>,
    socks_port: Option<u16>,
}

impl IntoIterator for TorArgs {
    type Item = String;
    type IntoIter = std::vec::IntoIter<String>;

    fn into_iter(self) -> Self::IntoIter {
        let mut args = Vec::new();

        if let Some(port) = self.socks_port {
            args.push(format!("SocksPort"));
            args.push(format!("0.0.0.0:{}", port));
        }

        if let Some(port) = self.control_port {
            args.push(format!("ControlPort"));
            args.push(format!("0.0.0.0:{}", port));
            args.push(format!("HashedControlPassword"));
            args.push(format!(
                "16:436B425404AA332A60B4F341C2023146C4B3A80548D757F0BB10DE81B4"
            ))
        }

        args.into_iter()
    }
}

impl Image for TorImage {
    type Args = TorArgs;
    type EnvVars = HashMap<String, String>;
    type Volumes = HashMap<String, String>;
    type EntryPoint = Infallible;

    fn descriptor(&self) -> String {
        "testcontainers-tor:latest".to_owned() // this is build locally using
                                               // the buildscript
    }

    fn wait_until_ready<D: Docker>(&self, container: &Container<'_, D, Self>) {
        container
            .logs()
            .stdout
            .wait_for_message("Bootstrapped 100% (done): Done")
            .unwrap();
    }

    fn args(&self) -> Self::Args {
        self.args.clone()
    }

    fn env_vars(&self) -> Self::EnvVars {
        HashMap::new()
    }

    fn volumes(&self) -> Self::Volumes {
        HashMap::new()
    }

    fn with_args(self, args: Self::Args) -> Self {
        Self { args }
    }
}
