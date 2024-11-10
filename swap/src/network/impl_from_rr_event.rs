/// Helper macro to map a [`request_response::Event`] to our [`OutEvent`].
///
/// This is primarily a macro and not a regular function because we use it for
/// Alice and Bob and they have different [`OutEvent`]s that just happen to
/// share a couple of variants, like `OutEvent::Failure` and `OutEvent::Other`.
#[macro_export]
macro_rules! impl_from_rr_event {
    ($protocol_event:ty, $behaviour_out_event:ty, $protocol:ident) => {
        impl From<$protocol_event> for $behaviour_out_event {
            fn from(event: $protocol_event) -> Self {
                use ::libp2p::request_response::Event::*;

                match event {
                    Message { message, peer, .. } => Self::from((peer, message)),
                    ResponseSent { .. } => Self::Other,
                    InboundFailure {
                        peer,
                        error,
                        request_id,
                    } => Self::InboundRequestResponseFailure {
                        peer,
                        error,
                        request_id,
                        protocol: $protocol.to_string(),
                    },
                    OutboundFailure {
                        peer,
                        error,
                        request_id,
                    } => Self::OutboundRequestResponseFailure {
                        peer,
                        error,
                        request_id,
                        protocol: $protocol.to_string(),
                    },
                }
            }
        }
    };
}
