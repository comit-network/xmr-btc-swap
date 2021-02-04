use crate::protocol::{
    alice::{SwapResponse, TransferProof},
    bob::{EncryptedSignature, SwapRequest},
};
use async_trait::async_trait;
use futures::prelude::*;
use libp2p::{
    core::{upgrade, upgrade::ReadOneError},
    request_response::{ProtocolName, RequestResponseCodec},
};
use serde::{Deserialize, Serialize};
use std::{fmt::Debug, io, marker::PhantomData};

/// Time to wait for a response back once we send a request.
pub const TIMEOUT: u64 = 3600; // One hour.

/// Message receive buffer.
pub const BUF_SIZE: usize = 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Request {
    SwapRequest(Box<SwapRequest>),
    TransferProof(Box<TransferProof>),
    EncryptedSignature(Box<EncryptedSignature>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Response {
    SwapResponse(Box<SwapResponse>),
    TransferProof,
    EncryptedSignature,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Swap;

#[derive(Debug, Clone, Copy, Default)]
pub struct TransferProofProtocol;

#[derive(Debug, Clone, Copy, Default)]
pub struct EncryptedSignatureProtocol;

impl ProtocolName for Swap {
    fn protocol_name(&self) -> &[u8] {
        b"/comit/xmr/btc/swap/1.0.0"
    }
}

impl ProtocolName for TransferProofProtocol {
    fn protocol_name(&self) -> &[u8] {
        b"/comit/xmr/btc/transfer_proof/1.0.0"
    }
}

impl ProtocolName for EncryptedSignatureProtocol {
    fn protocol_name(&self) -> &[u8] {
        b"/comit/xmr/btc/encrypted_signature/1.0.0"
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OneShotCodec<P> {
    phantom: PhantomData<P>,
}

#[async_trait]
impl<P> RequestResponseCodec for OneShotCodec<P>
where
    P: Send + Sync + Clone + ProtocolName,
{
    type Protocol = P;
    type Request = Request;
    type Response = Response;

    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let message = upgrade::read_one(io, BUF_SIZE).await.map_err(|e| match e {
            ReadOneError::Io(err) => err,
            e => io::Error::new(io::ErrorKind::Other, e),
        })?;
        let mut de = serde_cbor::Deserializer::from_slice(&message);
        let msg = Request::deserialize(&mut de).map_err(|e| {
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
        let message = upgrade::read_one(io, BUF_SIZE)
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut de = serde_cbor::Deserializer::from_slice(&message);
        let msg = Response::deserialize(&mut de).map_err(|e| {
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
        let bytes = serde_cbor::to_vec(&res).map_err(|e| {
            tracing::debug!("serde write_reponse error: {:?}", e);
            io::Error::new(io::ErrorKind::InvalidData, e)
        })?;
        upgrade::write_one(io, &bytes).await?;

        Ok(())
    }
}
