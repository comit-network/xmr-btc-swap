use crate::asb::{LatestRate, Rate};
use anyhow::Result;
use bitcoin::util::amount::ParseAmountError;
use futures::{SinkExt, StreamExt};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::convert::TryFrom;
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::{protocol::CloseFrame, Message};
use tracing::{error, trace};
use watch::Receiver;

const KRAKEN_WS_URL: &str = "wss://ws.kraken.com";
const SUBSCRIBE_XMR_BTC_TICKER_PAYLOAD: &str = r#"
{ "event": "subscribe",
  "pair": [ "XMR/XBT" ],
  "subscription": {
    "name": "ticker"
  }
}"#;

#[derive(Clone)]
pub struct RateService {
    receiver: Receiver<Result<Rate, Error>>,
}

impl LatestRate for RateService {
    type Error = Error;

    fn latest_rate(&mut self) -> Result<Rate, Self::Error> {
        (*self.receiver.borrow()).clone()
    }
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum Error {
    #[error("Rate has not yet been retrieved from Kraken websocket API")]
    NotYetRetrieved,
    #[error("Received close message from Kraken")]
    CloseMessage,
    #[error("Websocket: ")]
    WebSocket(String),
    #[error("Serde: ")]
    Serde(String),
    #[error("Data field is missing")]
    DataFieldMissing,
    #[error("Ask Rate Element is of unexpected type")]
    UnexpectedAskRateElementType,
    #[error("Ask Rate Element is missing")]
    MissingAskRateElementType,
    #[error("Bitcoin amount parse error: ")]
    BitcoinParseAmount(#[from] ParseAmountError),
}

impl From<tokio_tungstenite::tungstenite::Error> for Error {
    fn from(err: tokio_tungstenite::tungstenite::Error) -> Self {
        Error::WebSocket(format!("{:#}", err))
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Serde(format!("{:#}", err))
    }
}

impl RateService {
    pub async fn new() -> Result<Self> {
        let (rate_update, rate_update_receiver) = watch::channel(Err(Error::NotYetRetrieved));

        let (rate_stream, _response) =
            tokio_tungstenite::connect_async(Url::parse(KRAKEN_WS_URL).expect("valid url")).await?;

        let (mut rate_stream_sink, mut rate_stream) = rate_stream.split();

        tokio::spawn(async move {
            while let Some(msg) = rate_stream.next().await {
                let msg = match msg {
                    Ok(Message::Text(msg)) => msg,
                    Ok(Message::Close(close_frame)) => {
                        if let Some(CloseFrame { code, reason }) = close_frame {
                            error!(
                                "Kraken rate stream was closed with code {} and reason: {}",
                                code, reason
                            );
                        } else {
                            error!("Kraken rate stream was closed without code and reason");
                        }
                        let _ = rate_update.send(Err(Error::CloseMessage));
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
                        error!("Error when reading from Kraken rate stream: {}", e);
                        let _ = rate_update.send(Err(e.into()));
                        continue;
                    }
                };

                // If we encounter a heartbeat we skip it and iterate again
                if msg.eq(r#"{"event":"heartbeat"}"#) {
                    continue;
                }

                let ticker = match serde_json::from_str::<TickerUpdate>(&msg) {
                    Ok(ticker) => ticker,
                    Err(e) => {
                        let _ = rate_update.send(Err(e.into()));
                        continue;
                    }
                };

                let rate = match Rate::try_from(ticker) {
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

        Ok(Self {
            receiver: rate_update_receiver,
        })
    }
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

    #[tokio::test]
    async fn deserialize_ticker_update() {
        let sample_response = r#"[980,{"a":["0.00521900",4,"4.84775132"],"b":["0.00520600",70,"70.35668921"],"c":["0.00520700","0.00000186"],"v":["18530.40510860","18531.94887860"],"p":["0.00489493","0.00489490"],"t":[5017,5018],"l":["0.00448300","0.00448300"],"h":["0.00525000","0.00525000"],"o":["0.00450000","0.00451000"]},"ticker","XMR/XBT"]"#;

        let _ = serde_json::from_str::<TickerUpdate>(sample_response).unwrap();
    }
}
