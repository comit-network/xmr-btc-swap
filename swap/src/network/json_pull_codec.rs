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

/// A [`RequestResponseCodec`] for pull-based protocols where the response is
/// encoded using JSON.
///
/// A pull-based protocol is a protocol where the dialer doesn't send any
/// message and expects the listener to directly send the response as the
/// substream is opened.
#[derive(Clone, Copy, Debug)]
pub struct JsonPullCodec<P, Res> {
    phantom: PhantomData<(P, Res)>,
}

impl<P, Res> Default for JsonPullCodec<P, Res> {
    fn default() -> Self {
        Self {
            phantom: PhantomData::default(),
        }
    }
}

#[async_trait]
impl<P, Res> RequestResponseCodec for JsonPullCodec<P, Res>
where
    P: ProtocolName + Send + Sync + Clone,
    Res: DeserializeOwned + Serialize + Send,
{
    type Protocol = P;
    type Request = ();
    type Response = Res;

    async fn read_request<T>(&mut self, _: &Self::Protocol, _: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        Ok(())
    }

    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let message = upgrade::read_length_prefixed(io, BUF_SIZE)
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut de = serde_json::Deserializer::from_slice(&message);
        let msg = Res::deserialize(&mut de)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

        Ok(msg)
    }

    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        _: &mut T,
        _: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
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
        let bytes = serde_json::to_vec(&res)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        upgrade::write_length_prefixed(io, &bytes).await?;

        Ok(())
    }
}
