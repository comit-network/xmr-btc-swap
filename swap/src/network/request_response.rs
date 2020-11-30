use async_trait::async_trait;
use futures::prelude::*;
use libp2p::{
    core::upgrade,
    request_response::{ProtocolName, RequestResponseCodec},
};
use serde::{Deserialize, Serialize};
use std::{fmt::Debug, io, marker::PhantomData};
use tracing::debug;

use crate::SwapAmounts;
use xmr_btc::{alice, bob, monero};

/// Time to wait for a response back once we send a request.
pub const TIMEOUT: u64 = 3600; // One hour.

/// Message receive buffer.
const BUF_SIZE: usize = 1024 * 1024;

// TODO: Think about whether there is a better way to do this, e.g., separate
// Codec for each Message and a macro that implements them.

/// Messages Bob sends to Alice.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
pub enum BobToAlice {
    #[serde(with = "::bitcoin::util::amount::serde::as_sat")]
    AmountsFromBtc(::bitcoin::Amount),
    AmountsFromXmr(monero::Amount),
    Message0(bob::Message0),
    Message1(bob::Message1),
    Message2(bob::Message2),
    Message3(bob::Message3),
}

/// Messages Alice sends to Bob.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
pub enum AliceToBob {
    Amounts(SwapAmounts),
    Message0(alice::Message0),
    Message1(alice::Message1),
    Message2(alice::Message2),
    Message3, // empty response
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AmountsProtocol;

#[derive(Debug, Clone, Copy, Default)]
pub struct Message0Protocol;

#[derive(Debug, Clone, Copy, Default)]
pub struct Message1Protocol;

#[derive(Debug, Clone, Copy, Default)]
pub struct Message2Protocol;

#[derive(Debug, Clone, Copy, Default)]
pub struct Message3Protocol;

impl ProtocolName for AmountsProtocol {
    fn protocol_name(&self) -> &[u8] {
        b"/xmr/btc/amounts/1.0.0"
    }
}

impl ProtocolName for Message0Protocol {
    fn protocol_name(&self) -> &[u8] {
        b"/xmr/btc/message0/1.0.0"
    }
}

impl ProtocolName for Message1Protocol {
    fn protocol_name(&self) -> &[u8] {
        b"/xmr/btc/message1/1.0.0"
    }
}

impl ProtocolName for Message2Protocol {
    fn protocol_name(&self) -> &[u8] {
        b"/xmr/btc/message2/1.0.0"
    }
}

impl ProtocolName for Message3Protocol {
    fn protocol_name(&self) -> &[u8] {
        b"/xmr/btc/message3/1.0.0"
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Codec<P> {
    phantom: PhantomData<P>,
}

#[async_trait]
impl<P> RequestResponseCodec for Codec<P>
where
    P: Send + Sync + Clone + ProtocolName,
{
    type Protocol = P;
    type Request = BobToAlice;
    type Response = AliceToBob;

    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        debug!("enter read_request");
        let message = upgrade::read_one(io, BUF_SIZE)
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut de = serde_cbor::Deserializer::from_slice(&message);
        let msg = BobToAlice::deserialize(&mut de).map_err(|e| {
            tracing::debug!("serde read_request error: {:?}", e);
            io::Error::new(io::ErrorKind::Other, e)
        })?;

        Ok(msg)
    }

    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        debug!("enter read_response");
        let message = upgrade::read_one(io, BUF_SIZE)
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut de = serde_cbor::Deserializer::from_slice(&message);
        let msg = AliceToBob::deserialize(&mut de).map_err(|e| {
            tracing::debug!("serde read_response error: {:?}", e);
            io::Error::new(io::ErrorKind::InvalidData, e)
        })?;

        Ok(msg)
    }

    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let bytes =
            serde_cbor::to_vec(&req).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        upgrade::write_one(io, &bytes).await?;

        Ok(())
    }

    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        res: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        debug!("enter write_response");
        let bytes = serde_cbor::to_vec(&res).map_err(|e| {
            tracing::debug!("serde write_reponse error: {:?}", e);
            io::Error::new(io::ErrorKind::InvalidData, e)
        })?;
        upgrade::write_one(io, &bytes).await?;

        Ok(())
    }
}
