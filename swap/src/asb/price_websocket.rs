use crate::kraken::PriceUpdates;
use anyhow::Context;
use futures::{stream, StreamExt, TryStreamExt};
use warp::ws::{Message, WebSocket};

pub async fn latest_rate(ws: WebSocket, subscription: PriceUpdates) {
    let stream = stream::try_unfold(subscription.inner, |mut receiver| async move {
        // todo print error message but don't forward it to the user and don't panic
        receiver
            .changed()
            .await
            .context("failed to receive latest rate update")
            .expect("Should not fail :)");

        let latest_rate = receiver.borrow().clone().expect("Should work");

        // TODO: Proper definition of what to send over the wire
        // TODO: Properly calculate the actual rate (using spread) and add min and max
        // amounts tradeable
        let msg = Message::text(serde_json::to_string(&latest_rate).expect("to serialize"));

        Ok(Some((msg, receiver)))
    })
    .into_stream();

    let (ws_tx, mut _ws_rx) = ws.split();
    tokio::task::spawn(stream.forward(ws_tx));
}
