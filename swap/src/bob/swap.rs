use crate::{
    bob::{OutEvent, Swarm},
    state,
    storage::Database,
    Cmd, Rsp, PUNISH_TIMELOCK, REFUND_TIMELOCK,
};
use anyhow::Result;
use async_recursion::async_recursion;

use futures::{
    channel::mpsc::{Receiver, Sender},
    StreamExt,
};

use libp2p::PeerId;
use rand::rngs::OsRng;
use std::{process, sync::Arc};

use tracing::info;
use uuid::Uuid;
use xmr_btc::bob::{self, State0};

// The same data structure is used for swap execution and recovery.
// This allows for a seamless transition from a failed swap to recovery.
pub enum BobState {
    Started(Sender<Cmd>, Receiver<Rsp>, u64, PeerId),
    Negotiated(bob::State2, PeerId),
    BtcLocked(bob::State3, PeerId),
    XmrLocked(bob::State4, PeerId),
    EncSigSent(bob::State4, PeerId),
    BtcRedeemed(bob::State5),
    Cancelled(bob::State4),
    BtcRefunded,
    XmrRedeemed,
    Punished,
    SafelyAborted,
}

// State machine driver for swap execution
#[async_recursion]
pub async fn swap(
    state: BobState,
    mut swarm: Swarm,
    db: Database,
    bitcoin_wallet: Arc<crate::bitcoin::Wallet>,
    monero_wallet: Arc<crate::monero::Wallet>,
    mut rng: OsRng,
    swap_id: Uuid,
) -> Result<BobState> {
    match state {
        BobState::Started(mut cmd_tx, mut rsp_rx, btc, alice_peer_id) => {
            // todo: dial the swarm outside
            // libp2p::Swarm::dial_addr(&mut swarm, addr)?;
            let alice = match swarm.next().await {
                OutEvent::ConnectionEstablished(alice) => alice,
                other => panic!("unexpected event: {:?}", other),
            };
            info!("Connection established with: {}", alice);

            swarm.request_amounts(alice.clone(), btc);

            // todo: remove mspc channel
            let (btc, xmr) = match swarm.next().await {
                OutEvent::Amounts(amounts) => {
                    info!("Got amounts from Alice: {:?}", amounts);
                    let cmd = Cmd::VerifyAmounts(amounts);
                    cmd_tx.try_send(cmd)?;
                    let response = rsp_rx.next().await;
                    if response == Some(Rsp::Abort) {
                        info!("User rejected amounts proposed by Alice, aborting...");
                        process::exit(0);
                    }

                    info!("User accepted amounts proposed by Alice");
                    (amounts.btc, amounts.xmr)
                }
                other => panic!("unexpected event: {:?}", other),
            };

            let refund_address = bitcoin_wallet.new_address().await?;

            let state0 = State0::new(
                &mut rng,
                btc,
                xmr,
                REFUND_TIMELOCK,
                PUNISH_TIMELOCK,
                refund_address,
            );

            info!("Commencing handshake");

            swarm.send_message0(alice.clone(), state0.next_message(&mut rng));
            let state1 = match swarm.next().await {
                OutEvent::Message0(msg) => state0.receive(bitcoin_wallet.as_ref(), msg).await?,
                other => panic!("unexpected event: {:?}", other),
            };

            swarm.send_message1(alice.clone(), state1.next_message());
            let state2 = match swarm.next().await {
                OutEvent::Message1(msg) => state1.receive(msg)?,
                other => panic!("unexpected event: {:?}", other),
            };

            let swap_id = Uuid::new_v4();
            db.insert_latest_state(swap_id, state::Bob::Handshaken(state2.clone()).into())
                .await?;

            swarm.send_message2(alice.clone(), state2.next_message());

            info!("Handshake complete");

            swap(
                BobState::Negotiated(state2, alice_peer_id),
                swarm,
                db,
                bitcoin_wallet,
                monero_wallet,
                rng,
                swap_id,
            )
            .await
        }
        BobState::Negotiated(state2, alice_peer_id) => {
            // Alice and Bob have exchanged info
            let state3 = state2.lock_btc(bitcoin_wallet.as_ref()).await?;
            // db.insert_latest_state(state);
            swap(
                BobState::BtcLocked(state3, alice_peer_id),
                swarm,
                db,
                bitcoin_wallet,
                monero_wallet,
                rng,
                swap_id,
            )
            .await
        }
        // Bob has locked Btc
        // Watch for Alice to Lock Xmr or for t1 to elapse
        BobState::BtcLocked(state3, alice_peer_id) => {
            // todo: watch until t1, not indefinetely
            let state4 = match swarm.next().await {
                OutEvent::Message2(msg) => {
                    state3
                        .watch_for_lock_xmr(monero_wallet.as_ref(), msg)
                        .await?
                }
                other => panic!("unexpected event: {:?}", other),
            };
            swap(
                BobState::XmrLocked(state4, alice_peer_id),
                swarm,
                db,
                bitcoin_wallet,
                monero_wallet,
                rng,
                swap_id,
            )
            .await
        }
        BobState::XmrLocked(state, alice_peer_id) => {
            // Alice has locked Xmr
            // Bob sends Alice his key
            // let cloned = state.clone();
            let tx_redeem_encsig = state.tx_redeem_encsig();
            // Do we have to wait for a response?
            // What if Alice fails to receive this? Should we always resend?
            // todo: If we cannot dial Alice we should go to EncSigSent. Maybe dialing
            // should happen in this arm?
            swarm.send_message3(alice_peer_id.clone(), tx_redeem_encsig);

            swap(
                BobState::EncSigSent(state, alice_peer_id),
                swarm,
                db,
                bitcoin_wallet,
                monero_wallet,
                rng,
                swap_id,
            )
            .await
        }
        BobState::EncSigSent(state, ..) => {
            // Watch for redeem
            let redeem_watcher = state.watch_for_redeem_btc(bitcoin_wallet.as_ref());
            let t1_timeout = state.wait_for_t1(bitcoin_wallet.as_ref());

            tokio::select! {
                val = redeem_watcher => {
                    swap(
                        BobState::BtcRedeemed(val?),
                        swarm,
                        db,
                        bitcoin_wallet,
                        monero_wallet,
                                 rng,
                                 swap_id,
                    )
                    .await
                }
                _ = t1_timeout => {
                    // Check whether TxCancel has been published.
                    // We should not fail if the transaction is already on the blockchain
                    if let Err(_) = state.check_for_tx_cancel(bitcoin_wallet.as_ref()).await {
                            state.submit_tx_cancel(bitcoin_wallet.as_ref()).await?;
                    }

                    swap(
                        BobState::Cancelled(state),
                        swarm,
                        db,
                        bitcoin_wallet,
                        monero_wallet,
                        rng,
                 swap_id
                    )
                    .await

                }
            }
        }
        BobState::BtcRedeemed(state) => {
            // Bob redeems XMR using revealed s_a
            state.claim_xmr(monero_wallet.as_ref()).await?;
            swap(
                BobState::XmrRedeemed,
                swarm,
                db,
                bitcoin_wallet,
                monero_wallet,
                rng,
                swap_id,
            )
            .await
        }
        BobState::Cancelled(_state) => Ok(BobState::BtcRefunded),
        BobState::BtcRefunded => Ok(BobState::BtcRefunded),
        BobState::Punished => Ok(BobState::Punished),
        BobState::SafelyAborted => Ok(BobState::SafelyAborted),
        BobState::XmrRedeemed => Ok(BobState::XmrRedeemed),
    }
}

// // State machine driver for recovery execution
// #[async_recursion]
// pub async fn abort(state: BobState, io: Io) -> Result<BobState> {
//     match state {
//         BobState::Started => {
//             // Nothing has been commited by either party, abort swap.
//             abort(BobState::SafelyAborted, io).await
//         }
//         BobState::Negotiated => {
//             // Nothing has been commited by either party, abort swap.
//             abort(BobState::SafelyAborted, io).await
//         }
//         BobState::BtcLocked => {
//             // Bob has locked BTC and must refund it
//             // Bob waits for alice to publish TxRedeem or t1
//             if unimplemented!("TxRedeemSeen") {
//                 // Alice has redeemed revealing s_a
//                 abort(BobState::BtcRedeemed, io).await
//             } else if unimplemented!("T1Elapsed") {
//                 // publish TxCancel or see if it has been published
//                 abort(BobState::Cancelled, io).await
//             } else {
//                 Err(unimplemented!())
//             }
//         }
//         BobState::XmrLocked => {
//             // Alice has locked Xmr
//             // Wait until t1
//             if unimplemented!(">t1 and <t2") {
//                 // Bob publishes TxCancel
//                 abort(BobState::Cancelled, io).await
//             } else {
//                 // >t2
//                 // submit TxCancel
//                 abort(BobState::Punished, io).await
//             }
//         }
//         BobState::Cancelled => {
//             // Bob has cancelled the swap
//             // If <t2 Bob refunds
//             if unimplemented!("<t2") {
//                 // Submit TxRefund
//                 abort(BobState::BtcRefunded, io).await
//             } else {
//                 // Bob failed to refund in time and has been punished
//                 abort(BobState::Punished, io).await
//             }
//         }
//         BobState::BtcRedeemed => {
//             // Bob uses revealed s_a to redeem XMR
//             abort(BobState::XmrRedeemed, io).await
//         }
//         BobState::BtcRefunded => Ok(BobState::BtcRefunded),
//         BobState::Punished => Ok(BobState::Punished),
//         BobState::SafelyAborted => Ok(BobState::SafelyAborted),
//         BobState::XmrRedeemed => Ok(BobState::XmrRedeemed),
//     }
// }
