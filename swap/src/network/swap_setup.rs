pub mod protocol {
    use futures::future;
    use libp2p::core::upgrade::{from_fn, FromFnUpgrade};
    use libp2p::core::Endpoint;
    use libp2p::swarm::NegotiatedSubstream;
    use void::Void;

    pub fn new() -> SwapSetup {
        from_fn(
            b"/comit/xmr/btc/swap_setup/1.0.0",
            Box::new(|socket, _| future::ready(Ok(socket))),
        )
    }

    pub type SwapSetup = FromFnUpgrade<
        &'static [u8],
        Box<
            dyn Fn(
                    NegotiatedSubstream,
                    Endpoint,
                ) -> future::Ready<Result<NegotiatedSubstream, Void>>
                + Send
                + 'static,
        >,
    >;
}
