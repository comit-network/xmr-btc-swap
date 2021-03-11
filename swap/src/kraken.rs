use crate::asb::Rate;
use anyhow::{anyhow, Context, Result};
use futures::{SinkExt, StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use std::convert::{Infallible, TryFrom};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// Connect to Kraken websocket API for a constant stream of rate updates.
///
/// If the connection fails, it will automatically be re-established.
pub fn connect() -> Result<RateUpdateStream> {
    let (rate_update, rate_update_receiver) = watch::channel(Err(Error::NotYetAvailable));
    let rate_update = Arc::new(rate_update);

    tokio::spawn(async move {
        // The default backoff config is fine for us apart from one thing:
        // `max_elapsed_time`. If we don't get an error within this timeframe,
        // backoff won't actually retry the operation.
        let backoff = backoff::ExponentialBackoff {
            max_elapsed_time: None,
            ..backoff::ExponentialBackoff::default()
        };

        let result = backoff::future::retry_notify::<Infallible, _, _, _, _, _>(
            backoff,
            || {
                let rate_update = rate_update.clone();
                async move {
                    let mut stream = connection::new().await?;

                    while let Some(update) = stream.try_next().await.map_err(to_backoff)? {
                        let send_result = rate_update.send(Ok(update));

                        if send_result.is_err() {
                            return Err(backoff::Error::Permanent(anyhow!(
                                "receiver disconnected"
                            )));
                        }
                    }

                    Err(backoff::Error::Transient(anyhow!("stream ended")))
                }
            },
            |error, next: Duration| {
                tracing::info!(%error, "Kraken websocket connection failed, retrying in {}ms", next.as_millis());
            }
        )
        .await;

        match result {
            Err(e) => {
                tracing::warn!("Rate updates incurred an unrecoverable error: {:#}", e);

                // in case the retries fail permanently, let the subscribers know
                rate_update.send(Err(Error::PermanentFailure))
            }
            Ok(never) => match never {},
        }
    });

    Ok(RateUpdateStream {
        inner: rate_update_receiver,
    })
}

#[derive(Clone, Debug)]
pub struct RateUpdateStream {
    inner: watch::Receiver<RateUpdate>,
}

impl RateUpdateStream {
    pub async fn wait_for_update(&mut self) -> Result<RateUpdate> {
        self.inner.changed().await?;

        Ok(self.inner.borrow().clone())
    }

    pub fn latest_update(&mut self) -> RateUpdate {
        self.inner.borrow().clone()
    }
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum Error {
    #[error("Rate is not yet available")]
    NotYetAvailable,
    #[error("Permanently failed to retrieve rate from Kraken")]
    PermanentFailure,
}

type RateUpdate = Result<Rate, Error>;

/// Maps a [`connection::Error`] to a backoff error, effectively defining our
/// retry strategy.
fn to_backoff(e: connection::Error) -> backoff::Error<anyhow::Error> {
    use backoff::Error::*;

    match e {
        // Connection closures and websocket errors will be retried
        connection::Error::ConnectionClosed => Transient(anyhow::Error::from(e)),
        connection::Error::WebSocket(_) => Transient(anyhow::Error::from(e)),

        // Failures while parsing a message are permanent because they most likely present a
        // programmer error
        connection::Error::Parse(_) => Permanent(anyhow::Error::from(e)),
    }
}

/// Kraken websocket connection module.
///
/// Responsible for establishing a connection to the Kraken websocket API and
/// transforming the received websocket frames into a stream of rate updates.
/// The connection may fail in which case it is simply terminated and the stream
/// ends.
mod connection {
    use super::*;
    use crate::kraken::wire;
    use futures::stream::BoxStream;
    use tokio_tungstenite::tungstenite;

    pub async fn new() -> Result<BoxStream<'static, Result<Rate, Error>>> {
        let (mut rate_stream, _) = tokio_tungstenite::connect_async("wss://ws.kraken.com")
            .await
            .context("Failed to connect to Kraken websocket API")?;

        rate_stream
            .send(SUBSCRIBE_XMR_BTC_TICKER_PAYLOAD.into())
            .await?;

        let stream = rate_stream.err_into().try_filter_map(parse_message).boxed();

        Ok(stream)
    }

    /// Parse a websocket message into a [`Rate`].
    ///
    /// Messages which are not actually ticker updates are ignored and result in
    /// `None` being returned. In the context of a [`TryStream`], these will
    /// simply be filtered out.
    async fn parse_message(msg: tungstenite::Message) -> Result<Option<Rate>, Error> {
        let msg = match msg {
            tungstenite::Message::Text(msg) => msg,
            tungstenite::Message::Close(close_frame) => {
                if let Some(tungstenite::protocol::CloseFrame { code, reason }) = close_frame {
                    tracing::debug!(
                        "Kraken rate stream was closed with code {} and reason: {}",
                        code,
                        reason
                    );
                } else {
                    tracing::debug!("Kraken rate stream was closed without code and reason");
                }

                return Err(Error::ConnectionClosed);
            }
            msg => {
                tracing::trace!(
                    "Kraken rate stream returned non text message that will be ignored: {}",
                    msg
                );

                return Ok(None);
            }
        };

        let update = match serde_json::from_str::<wire::Event>(&msg) {
            Ok(wire::Event::SystemStatus) => {
                tracing::debug!("Connected to Kraken websocket API");

                return Ok(None);
            }
            Ok(wire::Event::SubscriptionStatus) => {
                tracing::debug!("Subscribed to updates for ticker");

                return Ok(None);
            }
            Ok(wire::Event::Heartbeat) => {
                tracing::trace!("Received heartbeat message");

                return Ok(None);
            }
            // if the message is not an event, it is a ticker update or an unknown event
            Err(_) => match serde_json::from_str::<wire::TickerUpdate>(&msg) {
                Ok(ticker) => ticker,
                Err(e) => {
                    tracing::warn!(%e, "Failed to deserialize message '{}' as ticker update", msg);

                    return Ok(None);
                }
            },
        };

        let update = Rate::try_from(update)?;

        Ok(Some(update))
    }

    #[derive(Debug, thiserror::Error)]
    pub enum Error {
        #[error("The Kraken server closed the websocket connection")]
        ConnectionClosed,
        #[error("Failed to read message from websocket stream")]
        WebSocket(#[from] tungstenite::Error),
        #[error("Failed to parse rate from websocket message")]
        Parse(#[from] wire::Error),
    }

    const SUBSCRIBE_XMR_BTC_TICKER_PAYLOAD: &str = r#"
    { "event": "subscribe",
      "pair": [ "XMR/XBT" ],
      "subscription": {
        "name": "ticker"
      }
    }"#;
}

/// Kraken websocket API wire module.
///
/// Responsible for parsing websocket text messages to events and rate updates.
mod wire {
    use super::*;
    use bitcoin::util::amount::ParseAmountError;
    use serde_json::Value;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    #[serde(tag = "event")]
    pub enum Event {
        #[serde(rename = "systemStatus")]
        SystemStatus,
        #[serde(rename = "heartbeat")]
        Heartbeat,
        #[serde(rename = "subscriptionStatus")]
        SubscriptionStatus,
    }

    #[derive(Clone, Debug, thiserror::Error)]
    pub enum Error {
        #[error("Data field is missing")]
        DataFieldMissing,
        #[error("Ask Rate Element is of unexpected type")]
        UnexpectedAskRateElementType,
        #[error("Ask Rate Element is missing")]
        MissingAskRateElementType,
        #[error("Failed to parse Bitcoin amount")]
        BitcoinParseAmount(#[from] ParseAmountError),
    }

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct TickerUpdate(Vec<TickerField>);

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(untagged)]
    pub enum TickerField {
        Data(TickerData),
        Metadata(Value),
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct TickerData {
        #[serde(rename = "a")]
        ask: Vec<RateElement>,
        #[serde(rename = "b")]
        bid: Vec<RateElement>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(untagged)]
    pub enum RateElement {
        Text(String),
        Number(u64),
    }

    impl TryFrom<TickerUpdate> for Rate {
        type Error = Error;

        fn try_from(value: TickerUpdate) -> Result<Self, Error> {
            let data = value
                .0
                .iter()
                .find_map(|field| match field {
                    TickerField::Data(data) => Some(data),
                    TickerField::Metadata(_) => None,
                })
                .ok_or(Error::DataFieldMissing)?;
            let ask = data.ask.first().ok_or(Error::MissingAskRateElementType)?;
            let ask = match ask {
                RateElement::Text(ask) => {
                    bitcoin::Amount::from_str_in(ask, ::bitcoin::Denomination::Bitcoin)?
                }
                _ => return Err(Error::UnexpectedAskRateElementType),
            };

            Ok(Self { ask })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn can_deserialize_system_status_event() {
            let event = r#"{"connectionID":14859574189081089471,"event":"systemStatus","status":"online","version":"1.8.1"}"#;

            let event = serde_json::from_str::<Event>(event).unwrap();

            assert_eq!(event, Event::SystemStatus)
        }

        #[test]
        fn can_deserialize_subscription_status_event() {
            let event = r#"{"channelID":980,"channelName":"ticker","event":"subscriptionStatus","pair":"XMR/XBT","status":"subscribed","subscription":{"name":"ticker"}}"#;

            let event = serde_json::from_str::<Event>(event).unwrap();

            assert_eq!(event, Event::SubscriptionStatus)
        }

        #[test]
        fn deserialize_ticker_update() {
            let message = r#"[980,{"a":["0.00440700",7,"7.35318535"],"b":["0.00440200",7,"7.57416678"],"c":["0.00440700","0.22579000"],"v":["273.75489000","4049.91233351"],"p":["0.00446205","0.00441699"],"t":[123,1310],"l":["0.00439400","0.00429900"],"h":["0.00450000","0.00450000"],"o":["0.00449100","0.00433700"]},"ticker","XMR/XBT"]"#;

            let _ = serde_json::from_str::<TickerUpdate>(message).unwrap();
        }
    }
}
