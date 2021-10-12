use async_trait::async_trait;
use futures::prelude::*;
use libp2p::core::upgrade;
use libp2p::request_response::{ProtocolName, RequestResponseCodec};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fmt::Debug;
use std::io;
use std::marker::PhantomData;

/// Message receive buffer.
pub const BUF_SIZE: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct CborCodec<P, Req, Res> {
    phantom: PhantomData<(P, Req, Res)>,
}

impl<P, Req, Res> Default for CborCodec<P, Req, Res> {
    fn default() -> Self {
        Self {
            phantom: PhantomData::default(),
        }
    }
}

#[async_trait]
impl<P, Req, Res> RequestResponseCodec for CborCodec<P, Req, Res>
where
    P: ProtocolName + Send + Sync + Clone,
    Req: DeserializeOwned + Serialize + Send,
    Res: DeserializeOwned + Serialize + Send,
{
    type Protocol = P;
    type Request = Req;
    type Response = Res;

    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let message = upgrade::read_length_prefixed(io, BUF_SIZE).await?;
        let mut de = serde_cbor::Deserializer::from_slice(&message);
        let msg = Req::deserialize(&mut de)
            .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;

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
        let message = upgrade::read_length_prefixed(io, BUF_SIZE).await?;
        let mut de = serde_cbor::Deserializer::from_slice(&message);
        let msg = Res::deserialize(&mut de)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

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

        upgrade::write_length_prefixed(io, &bytes).await?;

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
        let bytes = serde_cbor::to_vec(&res)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        upgrade::write_length_prefixed(io, &bytes).await?;

        Ok(())
    }
}
