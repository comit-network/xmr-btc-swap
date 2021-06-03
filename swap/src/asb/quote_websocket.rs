use crate::asb::Rate;
use crate::kraken::PriceUpdates;
use crate::network::quote::BidQuote;
use futures::{stream, StreamExt, TryStreamExt};
use rust_decimal::Decimal;
use serde::Serialize;
use warp::ws::{Message, WebSocket};
use warp::Filter;

pub async fn setup_quote_websocket(
    price_updates: PriceUpdates,
    port: u16,
    spread: Decimal,
    min: bitcoin::Amount,
    max: bitcoin::Amount,
) {
    let latest_quote = warp::get()
        .and(warp::path!("api" / "quote" / "xmr-btc"))
        .and(warp::ws())
        .map(move |ws: warp::ws::Ws| {
            let price_updates = price_updates.clone();
            tracing::info!("New quote websocket connection");
            ws.on_upgrade(move |socket| quote_stream(socket, price_updates, spread, min, max))
        });
    tokio::spawn(async move {
        warp::serve(latest_quote).run(([0, 0, 0, 0], port)).await;
    });
}

async fn quote_stream(
    ws: WebSocket,
    subscription: PriceUpdates,
    spread: Decimal,
    min: bitcoin::Amount,
    max: bitcoin::Amount,
) {
    let stream = stream::try_unfold(subscription.inner, move |mut receiver| async move {
        if let Err(e) = receiver.changed().await {
            tracing::error!(
                "Failed to initialize price update stream for quote websocket: {:#}",
                e
            );
        }

        let quote = match receiver.borrow().clone() {
            Ok(latest_price_update) => match Rate::new(latest_price_update.ask, spread).ask() {
                Ok(amount) => WebsocketBidQuote::Quote(BidQuote {
                    price: amount,
                    min_quantity: min,
                    max_quantity: max,
                }),
                Err(e) => {
                    tracing::error!("Failed to create quote for quote websocket: {:#}", e);
                    WebsocketBidQuote::Error(Error::BidQuoteError)
                }
            },
            Err(e) => {
                tracing::error!(
                    "Failed to fetch latest price update for quote websocket: {:#}",
                    e
                );
                WebsocketBidQuote::Error(Error::PriceUpdateError)
            }
        };

        let msg = Message::text(serde_json::to_string(&quote).expect("quote to serialize"));

        Ok(Some((msg, receiver)))
    })
    .into_stream();

    let (ws_tx, mut _ws_rx) = ws.split();
    tokio::task::spawn(stream.forward(ws_tx));
}

#[derive(Serialize, Debug, Clone)]
enum WebsocketBidQuote {
    Quote(BidQuote),
    Error(Error),
}

#[derive(Clone, Debug, Serialize)]
enum Error {
    PriceUpdateError,
    BidQuoteError,
}
