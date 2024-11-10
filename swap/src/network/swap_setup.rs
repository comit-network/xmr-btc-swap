use crate::monero;
use anyhow::{Context, Result};
use asynchronous_codec::{Bytes, Framed};
use futures::{SinkExt, StreamExt};

use libp2p::swarm::Stream;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub mod alice;
pub mod bob;
mod vendor_from_fn;

pub const BUF_SIZE: usize = 1024 * 1024;

pub mod protocol {
    use futures::future;
    use libp2p::core::Endpoint;
    use libp2p::swarm::Stream;
    use void::Void;

    use super::vendor_from_fn::{from_fn, FromFnUpgrade};

    pub fn new() -> SwapSetup {
        from_fn(
            "/comit/xmr/btc/swap_setup/1.0.0",
            Box::new(|socket, _| future::ready(Ok(socket))),
        )
    }

    pub type SwapSetup = FromFnUpgrade<
        &'static str,
        Box<dyn Fn(Stream, Endpoint) -> future::Ready<Result<Stream, Void>> + Send + 'static>,
    >;
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockchainNetwork {
    #[serde(with = "crate::bitcoin::network")]
    pub bitcoin: bitcoin::Network,
    #[serde(with = "crate::monero::network")]
    pub monero: monero::Network,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpotPriceRequest {
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    pub btc: bitcoin::Amount,
    pub blockchain_network: BlockchainNetwork,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SpotPriceResponse {
    Xmr(monero::Amount),
    Error(SpotPriceError),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SpotPriceError {
    NoSwapsAccepted,
    AmountBelowMinimum {
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        min: bitcoin::Amount,
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        buy: bitcoin::Amount,
    },
    AmountAboveMaximum {
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        max: bitcoin::Amount,
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        buy: bitcoin::Amount,
    },
    BalanceTooLow {
        #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
        buy: bitcoin::Amount,
    },
    BlockchainNetworkMismatch {
        cli: BlockchainNetwork,
        asb: BlockchainNetwork,
    },
    /// To be used for errors that cannot be explained on the CLI side (e.g.
    /// rate update problems on the seller side)
    Other,
}

fn codec() -> unsigned_varint::codec::UviBytes<Bytes> {
    let mut codec = unsigned_varint::codec::UviBytes::<Bytes>::default();
    codec.set_max_len(BUF_SIZE);
    codec
}

pub async fn read_cbor_message<T>(stream: &mut Stream) -> Result<T>
where
    T: DeserializeOwned,
{
    let mut frame = Framed::new(stream, codec());

    let bytes = frame
        .next()
        .await
        .context("Failed to read length-prefixed message from stream")??;

    let mut de = serde_cbor::Deserializer::from_slice(&bytes);
    let message =
        T::deserialize(&mut de).context("Failed to deserialize bytes into message using CBOR")?;

    Ok(message)
}

pub async fn write_cbor_message<T>(stream: &mut Stream, message: T) -> Result<()>
where
    T: Serialize,
{
    let bytes =
        serde_cbor::to_vec(&message).context("Failed to serialize message as bytes using CBOR")?;

    let mut frame = Framed::new(stream, codec());

    frame
        .send(Bytes::from(bytes))
        .await
        .context("Failed to write bytes as length-prefixed message")?;

    Ok(())
}
