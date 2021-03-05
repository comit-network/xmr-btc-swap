use crate::asb::Rate;
use anyhow::Result;
use bitcoin::util::amount::ParseAmountError;
use futures::{SinkExt, StreamExt};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::convert::TryFrom;
use tokio::sync::watch;
use tokio_tungstenite::tungstenite;
use tracing::{error, trace};

pub async fn connect() -> Result<watch::Receiver<Result<Rate, Error>>> {
    let (rate_update, rate_update_receiver) = watch::channel(Err(Error::NotYetRetrieved));

    let (rate_stream, _response) =
        tokio_tungstenite::connect_async(Url::parse(KRAKEN_WS_URL).expect("valid url")).await?;

    let (mut rate_stream_sink, mut rate_stream) = rate_stream.split();

    tokio::spawn(async move {
        while let Some(msg) = rate_stream.next().await {
            let msg = match msg {
                Ok(tungstenite::Message::Text(msg)) => msg,
                Ok(tungstenite::Message::Close(close_frame)) => {
                    if let Some(tungstenite::protocol::CloseFrame { code, reason }) = close_frame {
                        error!(
                            "Kraken rate stream was closed with code {} and reason: {}",
                            code, reason
                        );
                    } else {
                        error!("Kraken rate stream was closed without code and reason");
                    }
                    let _ = rate_update.send(Err(Error::ConnectionClosed));
                    continue;
                }
                Ok(msg) => {
                    trace!(
                        "Kraken rate stream returned non text message that will be ignored: {}",
                        msg
                    );
                    continue;
                }
                Err(e) => {
                    error!(%e, "Error when reading from Kraken rate stream");
                    let _ = rate_update.send(Err(e.into()));
                    continue;
                }
            };

            let update = match serde_json::from_str::<Event>(&msg) {
                Ok(Event::SystemStatus) => {
                    tracing::debug!("Connected to Kraken websocket API");
                    continue;
                }
                Ok(Event::SubscriptionStatus) => {
                    tracing::debug!("Subscribed to updates for ticker");
                    continue;
                }
                Ok(Event::Heartbeat) => {
                    tracing::trace!("Received heartbeat message");
                    continue;
                }
                // if the message is not an event, it is a ticker update or an unknown event
                Err(_) => match serde_json::from_str::<TickerUpdate>(&msg) {
                    Ok(ticker) => ticker,
                    Err(e) => {
                        tracing::warn!(%e, "Failed to deserialize message '{}' as ticker update", msg);
                        let _ = rate_update.send(Err(Error::UnknownMessage { msg }));
                        continue;
                    }
                },
            };

            let rate = match Rate::try_from(update) {
                Ok(rate) => rate,
                Err(e) => {
                    let _ = rate_update.send(Err(e));
                    continue;
                }
            };

            let _ = rate_update.send(Ok(rate));
        }
    });

    rate_stream_sink
        .send(SUBSCRIBE_XMR_BTC_TICKER_PAYLOAD.into())
        .await?;

    Ok(rate_update_receiver)
}

const KRAKEN_WS_URL: &str = "wss://ws.kraken.com";
const SUBSCRIBE_XMR_BTC_TICKER_PAYLOAD: &str = r#"
{ "event": "subscribe",
  "pair": [ "XMR/XBT" ],
  "subscription": {
    "name": "ticker"
  }
}"#;

#[derive(Clone, Debug, thiserror::Error)]
pub enum Error {
    #[error("Rate has not yet been retrieved from Kraken websocket API")]
    NotYetRetrieved,
    #[error("The Kraken server closed the websocket connection")]
    ConnectionClosed,
    #[error("Websocket: {0}")]
    WebSocket(String),
    #[error("Received unknown message from Kraken: {msg}")]
    UnknownMessage { msg: String },
    #[error("Data field is missing")]
    DataFieldMissing,
    #[error("Ask Rate Element is of unexpected type")]
    UnexpectedAskRateElementType,
    #[error("Ask Rate Element is missing")]
    MissingAskRateElementType,
    #[error("Bitcoin amount parse error: ")]
    BitcoinParseAmount(#[from] ParseAmountError),
}

impl From<tungstenite::Error> for Error {
    fn from(err: tungstenite::Error) -> Self {
        Error::WebSocket(format!("{:#}", err))
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "event")]
enum Event {
    #[serde(rename = "systemStatus")]
    SystemStatus,
    #[serde(rename = "heartbeat")]
    Heartbeat,
    #[serde(rename = "subscriptionStatus")]
    SubscriptionStatus,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
struct TickerUpdate(Vec<TickerField>);

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum TickerField {
    Data(TickerData),
    Metadata(Value),
}

#[derive(Debug, Serialize, Deserialize)]
struct TickerData {
    #[serde(rename = "a")]
    ask: Vec<RateElement>,
    #[serde(rename = "b")]
    bid: Vec<RateElement>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum RateElement {
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
