use crate::asb::{LatestRate, Rate};
use anyhow::{anyhow, bail, Result};
use futures::{SinkExt, StreamExt};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::convert::TryFrom;
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::Message;
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
    receiver: Receiver<Rate>,
}

impl LatestRate for RateService {
    fn latest_rate(&mut self) -> Rate {
        *self.receiver.borrow()
    }
}

impl RateService {
    pub async fn new() -> Result<Self> {
        let (tx, rx) = watch::channel(Rate::ZERO);

        let (ws, _response) =
            tokio_tungstenite::connect_async(Url::parse(KRAKEN_WS_URL).expect("valid url")).await?;

        let (mut write, mut read) = ws.split();

        // TODO: Handle the possibility of losing the connection
        // to the Kraken WS. Currently the stream would produce no
        // further items, and consumers would assume that the rate
        // is up to date
        tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                let msg = match msg {
                    Ok(Message::Text(msg)) => msg,
                    _ => continue,
                };

                let ticker = match serde_json::from_str::<TickerUpdate>(&msg) {
                    Ok(ticker) => ticker,
                    _ => continue,
                };

                let rate = match Rate::try_from(ticker) {
                    Ok(rate) => rate,
                    Err(e) => {
                        log::error!("could not get rate from ticker update: {}", e);
                        continue;
                    }
                };

                let _ = tx.send(rate);
            }
        });

        write.send(SUBSCRIBE_XMR_BTC_TICKER_PAYLOAD.into()).await?;

        Ok(Self { receiver: rx })
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
    type Error = anyhow::Error;

    fn try_from(value: TickerUpdate) -> Result<Self> {
        let data = value
            .0
            .iter()
            .find_map(|field| match field {
                TickerField::Data(data) => Some(data),
                TickerField::Metadata(_) => None,
            })
            .ok_or_else(|| anyhow!("ticker update does not contain data"))?;

        let ask = data.ask.first().ok_or_else(|| anyhow!("no ask price"))?;
        let ask = match ask {
            RateElement::Text(ask) => {
                bitcoin::Amount::from_str_in(ask, ::bitcoin::Denomination::Bitcoin)?
            }
            _ => bail!("unexpected ask rate element"),
        };

        Ok(Self { ask })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn deserialize_ticker_update() {
        let sample_response = r#"
[2308,{"a":["18215.60000",0,"0.27454523"],"b":["18197.50000",0,"0.63711255"],"c":["18197.50000","0.00413060"],"v":["2.78915585","156.15766485"],"p":["18200.94036","18275.19149"],"t":[22,1561],"l":["18162.40000","17944.90000"],"h":["18220.90000","18482.60000"],"o":["18220.90000","18478.90000"]},"ticker","XBT/USDT"]"#;

        let _ = serde_json::from_str::<TickerUpdate>(sample_response).unwrap();
    }
}
