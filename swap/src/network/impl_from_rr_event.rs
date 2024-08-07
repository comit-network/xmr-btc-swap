/// Helper macro to map a [`RequestResponseEvent`] to our [`OutEvent`].
///
/// This is primarily a macro and not a regular function because we use it for
/// Alice and Bob and they have different [`OutEvent`]s that just happen to
/// share a couple of variants, like `OutEvent::Failure` and `OutEvent::Other`.
#[macro_export]
macro_rules! impl_from_rr_event {
    ($protocol_event:ty, $behaviour_out_event:ty, $protocol:ident) => {
        impl From<$protocol_event> for $behaviour_out_event {
            fn from(event: $protocol_event) -> Self {
                use ::libp2p::request_response::RequestResponseEvent::*;
                use anyhow::anyhow;

                match event {
                    Message { message, peer, .. } => Self::from((peer, message)),
                    ResponseSent { .. } => Self::Other,
                    InboundFailure { peer, error, .. } => {
                        use libp2p::request_response::InboundFailure::*;

                        match error {
                            Timeout => {
                                Self::Failure {
                                    error: anyhow!("{} failed because of an inbound timeout", $protocol),
                                    peer,
                                }
                            }
                            ConnectionClosed => {
                                Self::Failure {
                                    error: anyhow!("{} failed because the connection was closed before a response could be sent", $protocol),
                                    peer,
                                }
                            }
                            UnsupportedProtocols => Self::Other, // TODO: Report this and disconnected / ban the peer?
                            ResponseOmission => Self::Other
                        }
                    }
                    OutboundFailure { peer, error, .. } => {
                        use libp2p::request_response::OutboundFailure::*;

                        match error {
                            Timeout => {
                                Self::Failure {
                                    error: anyhow!("{} failed because we did not receive a response within the configured timeout", $protocol),
                                    peer,
                                }
                            }
                            ConnectionClosed => {
                                Self::Failure {
                                    error: anyhow!("{} failed because the connection was closed we received a response", $protocol),
                                    peer,
                                }
                            }
                            UnsupportedProtocols => Self::Other, // TODO: Report this and disconnected / ban the peer?
                            DialFailure => {
                                Self::Failure {
                                    error: anyhow!("{} failed because we failed to dial", $protocol),
                                    peer,
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
