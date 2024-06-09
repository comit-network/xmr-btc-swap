use crate::api::Context;
use crate::bitcoin::{Amount, ExpiredTimelocks, TxLock};
use crate::cli::{list_sellers, EventLoop, SellerStatus};
use crate::libp2p_ext::MultiAddrExt;
use crate::network::quote::{BidQuote, ZeroQuoteReceived};
use crate::network::swarm;
use crate::protocol::bob::{BobState, Swap};
use crate::protocol::{bob, State};
use crate::{bitcoin, cli, monero, rpc};
use anyhow::{bail, Context as AnyContext, Result};
use libp2p::core::Multiaddr;
use qrcode::render::unicode;
use qrcode::QrCode;
use serde_json::json;
use std::cmp::min;
use std::convert::TryInto;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug_span, field, Instrument, Span};
use uuid::Uuid;

#[derive(PartialEq, Debug)]
pub struct Request {
    pub cmd: Method,
    pub log_reference: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum Method {
    BuyXmr {
        seller: Multiaddr,
        bitcoin_change_address: bitcoin::Address,
        monero_receive_address: monero::Address,
        swap_id: Uuid,
    },
    Resume {
        swap_id: Uuid,
    },
    CancelAndRefund {
        swap_id: Uuid,
    },
    MoneroRecovery {
        swap_id: Uuid,
    },
    History,
    Config,
    WithdrawBtc {
        amount: Option<Amount>,
        address: bitcoin::Address,
    },
    Balance {
        force_refresh: bool,
    },
    ListSellers {
        rendezvous_point: Multiaddr,
    },
    ExportBitcoinWallet,
    SuspendCurrentSwap,
    StartDaemon {
        server_address: Option<SocketAddr>,
    },
    GetCurrentSwap,
    GetSwapInfo {
        swap_id: Uuid,
    },
    GetRawStates,
    AttemptCooperativeRedeem {
        swap_id: Uuid,
    },
}

impl Method {
    fn get_tracing_span(&self, log_reference_id: Option<String>) -> Span {
        let span = match self {
            Method::AttemptCooperativeRedeem { swap_id } => {
                debug_span!("method", method_name="AttemptCooperativeRedeem", swap_id=%swap_id, log_reference_id=field::Empty)
            }
            Method::Balance { .. } => {
                debug_span!(
                    "method",
                    method_name = "Balance",
                    log_reference_id = field::Empty
                )
            }
            Method::BuyXmr { swap_id, .. } => {
                debug_span!("method", method_name="BuyXmr", swap_id=%swap_id, log_reference_id=field::Empty)
            }
            Method::CancelAndRefund { swap_id } => {
                debug_span!("method", method_name="CancelAndRefund", swap_id=%swap_id, log_reference_id=field::Empty)
            }
            Method::Resume { swap_id } => {
                debug_span!("method", method_name="Resume", swap_id=%swap_id, log_reference_id=field::Empty)
            }
            Method::Config => {
                debug_span!(
                    "method",
                    method_name = "Config",
                    log_reference_id = field::Empty
                )
            }
            Method::ExportBitcoinWallet => {
                debug_span!(
                    "method",
                    method_name = "ExportBitcoinWallet",
                    log_reference_id = field::Empty
                )
            }
            Method::GetCurrentSwap => {
                debug_span!(
                    "method",
                    method_name = "GetCurrentSwap",
                    log_reference_id = field::Empty
                )
            }
            Method::GetSwapInfo { .. } => {
                debug_span!(
                    "method",
                    method_name = "GetSwapInfo",
                    log_reference_id = field::Empty
                )
            }
            Method::History => {
                debug_span!(
                    "method",
                    method_name = "History",
                    log_reference_id = field::Empty
                )
            }
            Method::ListSellers { .. } => {
                debug_span!(
                    "method",
                    method_name = "ListSellers",
                    log_reference_id = field::Empty
                )
            }
            Method::MoneroRecovery { .. } => {
                debug_span!(
                    "method",
                    method_name = "MoneroRecovery",
                    log_reference_id = field::Empty
                )
            }
            Method::GetRawStates => debug_span!(
                "method",
                method_name = "RawHistory",
                log_reference_id = field::Empty
            ),
            Method::StartDaemon { .. } => {
                debug_span!(
                    "method",
                    method_name = "StartDaemon",
                    log_reference_id = field::Empty
                )
            }
            Method::SuspendCurrentSwap => {
                debug_span!(
                    "method",
                    method_name = "SuspendCurrentSwap",
                    log_reference_id = field::Empty
                )
            }
            Method::WithdrawBtc { .. } => {
                debug_span!(
                    "method",
                    method_name = "WithdrawBtc",
                    log_reference_id = field::Empty
                )
            }
        };
        if let Some(log_reference_id) = log_reference_id {
            span.record("log_reference_id", log_reference_id.as_str());
        }
        span
    }
}

impl Request {
    pub fn new(cmd: Method) -> Request {
        Request {
            cmd,
            log_reference: None,
        }
    }

    pub fn with_id(cmd: Method, id: Option<String>) -> Request {
        Request {
            cmd,
            log_reference: id,
        }
    }

    async fn handle_cmd(self, context: Arc<Context>) -> Result<serde_json::Value> {
        match self.cmd {
            Method::SuspendCurrentSwap => {
                let swap_id = context.swap_lock.get_current_swap_id().await;

                if let Some(id_value) = swap_id {
                    context.swap_lock.send_suspend_signal().await?;

                    Ok(json!({ "swapId": id_value }))
                } else {
                    bail!("No swap is currently running")
                }
            }
            Method::GetSwapInfo { swap_id } => {
                let bitcoin_wallet = context
                    .bitcoin_wallet
                    .as_ref()
                    .context("Could not get Bitcoin wallet")?;

                let state = context.db.get_state(swap_id).await?;
                let is_completed = state.swap_finished();

                let peerId = context
                    .db
                    .get_peer_id(swap_id)
                    .await
                    .with_context(|| "Could not get PeerID")?;

                let addresses = context
                    .db
                    .get_addresses(peerId)
                    .await
                    .with_context(|| "Could not get addressess")?;

                let start_date = context.db.get_swap_start_date(swap_id).await?;

                let swap_state: BobState = state.try_into()?;
                let state_name = format!("{}", swap_state);

                let (
                    xmr_amount,
                    btc_amount,
                    tx_lock_id,
                    tx_cancel_fee,
                    tx_refund_fee,
                    tx_lock_fee,
                    btc_refund_address,
                    cancel_timelock,
                    punish_timelock,
                ) = context
                    .db
                    .get_states(swap_id)
                    .await?
                    .iter()
                    .find_map(|state| {
                        if let State::Bob(BobState::SwapSetupCompleted(state2)) = state {
                            let xmr_amount = state2.xmr;
                            let btc_amount = state2.tx_lock.lock_amount().to_sat();
                            let tx_cancel_fee = state2.tx_cancel_fee.to_sat();
                            let tx_refund_fee = state2.tx_refund_fee.to_sat();
                            let tx_lock_id = state2.tx_lock.txid();
                            let btc_refund_address = state2.refund_address.to_string();

                            if let Ok(tx_lock_fee) = state2.tx_lock.fee() {
                                let tx_lock_fee = tx_lock_fee.to_sat();

                                Some((
                                    xmr_amount,
                                    btc_amount,
                                    tx_lock_id,
                                    tx_cancel_fee,
                                    tx_refund_fee,
                                    tx_lock_fee,
                                    btc_refund_address,
                                    state2.cancel_timelock,
                                    state2.punish_timelock,
                                ))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .with_context(|| "Did not find SwapSetupCompleted state for swap")?;

                let timelock = match swap_state {
                    BobState::Started { .. }
                    | BobState::SafelyAborted
                    | BobState::SwapSetupCompleted(_) => None,
                    BobState::BtcLocked { state3: state, .. }
                    | BobState::XmrLockProofReceived { state, .. } => {
                        Some(state.expired_timelock(bitcoin_wallet).await)
                    }
                    BobState::XmrLocked(state) | BobState::EncSigSent(state) => {
                        Some(state.expired_timelock(bitcoin_wallet).await)
                    }
                    BobState::CancelTimelockExpired(state) | BobState::BtcCancelled(state) => {
                        Some(state.expired_timelock(bitcoin_wallet).await)
                    }
                    BobState::BtcPunished { .. } => Some(Ok(ExpiredTimelocks::Punish)),
                    BobState::BtcRefunded { .. }
                    | BobState::BtcRedeemed(_)
                    | BobState::XmrRedeemed { .. } => None,
                };

                Ok(json!({
                    "swapId": swap_id,
                    "seller": {
                        "peerId": peerId.to_string(),
                        "addresses": addresses
                    },
                    "completed": is_completed,
                    "startDate": start_date,
                    "stateName": state_name,
                    "xmrAmount": xmr_amount,
                    "btcAmount": btc_amount,
                    "txLockId": tx_lock_id,
                    "txCancelFee": tx_cancel_fee,
                    "txRefundFee": tx_refund_fee,
                    "txLockFee": tx_lock_fee,
                    "btcRefundAddress": btc_refund_address.to_string(),
                    "cancelTimelock": cancel_timelock,
                    "punishTimelock": punish_timelock,
                    // If the timelock is None, it means that the swap is in a state where the timelock is not accessible to us.
                    // If that is the case, we return null. Otherwise, we return the timelock.
                    "timelock": timelock.map(|tl| tl.map(|tl| json!(tl)).unwrap_or(json!(null))).unwrap_or(json!(null)),
                }))
            }
            Method::BuyXmr {
                seller,
                bitcoin_change_address,
                monero_receive_address,
                swap_id,
            } => {
                let bitcoin_wallet = Arc::clone(
                    context
                        .bitcoin_wallet
                        .as_ref()
                        .expect("Could not find Bitcoin wallet"),
                );
                let monero_wallet = Arc::clone(
                    context
                        .monero_wallet
                        .as_ref()
                        .context("Could not get Monero wallet")?,
                );
                let env_config = context.config.env_config;
                let seed = context.config.seed.clone().context("Could not get seed")?;

                let seller_peer_id = seller
                    .extract_peer_id()
                    .context("Seller address must contain peer ID")?;
                context
                    .db
                    .insert_address(seller_peer_id, seller.clone())
                    .await?;

                let behaviour = cli::Behaviour::new(
                    seller_peer_id,
                    env_config,
                    bitcoin_wallet.clone(),
                    (seed.derive_libp2p_identity(), context.config.namespace),
                );
                let mut swarm = swarm::cli(
                    seed.derive_libp2p_identity(),
                    context.config.tor_socks5_port,
                    behaviour,
                )
                .await?;

                swarm.behaviour_mut().add_address(seller_peer_id, seller);

                context
                    .db
                    .insert_monero_address(swap_id, monero_receive_address)
                    .await?;

                tracing::debug!(peer_id = %swarm.local_peer_id(), "Network layer initialized");

                context.swap_lock.acquire_swap_lock(swap_id).await?;

                let initialize_swap = tokio::select! {
                    biased;
                    _ = context.swap_lock.listen_for_swap_force_suspension() => {
                        tracing::debug!("Shutdown signal received, exiting");
                        context.swap_lock.release_swap_lock().await.expect("Shutdown signal received but failed to release swap lock. The swap process has been terminated but the swap lock is still active.");
                        bail!("Shutdown signal received");
                    },
                    result = async {
                        let (event_loop, mut event_loop_handle) =
                            EventLoop::new(swap_id, swarm, seller_peer_id)?;
                        let event_loop = tokio::spawn(event_loop.run().in_current_span());

                        let bid_quote = event_loop_handle.request_quote().await?;

                        Ok::<_, anyhow::Error>((event_loop, event_loop_handle, bid_quote))
                    } => {
                        result
                    },
                };

                let (event_loop, event_loop_handle, bid_quote) = match initialize_swap {
                    Ok(result) => result,
                    Err(error) => {
                        tracing::error!(%swap_id, "Swap initialization failed: {:#}", error);
                        context
                            .swap_lock
                            .release_swap_lock()
                            .await
                            .expect("Could not release swap lock");
                        bail!(error);
                    }
                };

                context.tasks.clone().spawn(async move {
                    tokio::select! {
                        biased;
                        _ = context.swap_lock.listen_for_swap_force_suspension() => {
                            tracing::debug!("Shutdown signal received, exiting");
                            context.swap_lock.release_swap_lock().await.expect("Shutdown signal received but failed to release swap lock. The swap process has been terminated but the swap lock is still active.");
                            bail!("Shutdown signal received");
                        },
                        event_loop_result = event_loop => {
                            match event_loop_result {
                                Ok(_) => {
                                    tracing::debug!(%swap_id, "EventLoop completed")
                                }
                                Err(error) => {
                                    tracing::error!(%swap_id, "EventLoop failed: {:#}", error)
                                }
                            }
                        },
                        swap_result = async {
                            let max_givable = || bitcoin_wallet.max_giveable(TxLock::script_size());
                            let estimate_fee = |amount| bitcoin_wallet.estimate_fee(TxLock::weight(), amount);

                            let determine_amount = determine_btc_to_swap(
                                context.config.json,
                                bid_quote,
                                bitcoin_wallet.new_address(),
                                || bitcoin_wallet.balance(),
                                max_givable,
                                || bitcoin_wallet.sync(),
                                estimate_fee,
                            );

                            let (amount, fees) = match determine_amount.await {
                                Ok(val) => val,
                                Err(error) => match error.downcast::<ZeroQuoteReceived>() {
                                    Ok(_) => {
                                        bail!("Seller's XMR balance is currently too low to initiate a swap, please try again later")
                                    }
                                    Err(other) => bail!(other),
                                },
                            };

                            tracing::info!(%amount, %fees,  "Determined swap amount");

                            context.db.insert_peer_id(swap_id, seller_peer_id).await?;

                            let swap = Swap::new(
                                Arc::clone(&context.db),
                                swap_id,
                                Arc::clone(&bitcoin_wallet),
                                monero_wallet,
                                env_config,
                                event_loop_handle,
                                monero_receive_address,
                                bitcoin_change_address,
                                amount,
                            );

                            bob::run(swap).await
                        } => {
                            match swap_result {
                                Ok(state) => {
                                    tracing::debug!(%swap_id, state=%state, "Swap completed")
                                }
                                Err(error) => {
                                    tracing::error!(%swap_id, "Failed to complete swap: {:#}", error)
                                }
                            }
                        },
                    };
                    tracing::debug!(%swap_id, "Swap completed");

                    context
                        .swap_lock
                        .release_swap_lock()
                        .await
                        .expect("Could not release swap lock");
                    Ok::<_, anyhow::Error>(())
                }.in_current_span()).await;

                Ok(json!({
                    "swapId": swap_id.to_string(),
                    "quote": bid_quote,
                }))
            }
            Method::Resume { swap_id } => {
                context.swap_lock.acquire_swap_lock(swap_id).await?;

                let seller_peer_id = context.db.get_peer_id(swap_id).await?;
                let seller_addresses = context.db.get_addresses(seller_peer_id).await?;

                let seed = context
                    .config
                    .seed
                    .as_ref()
                    .context("Could not get seed")?
                    .derive_libp2p_identity();

                let behaviour = cli::Behaviour::new(
                    seller_peer_id,
                    context.config.env_config,
                    Arc::clone(
                        context
                            .bitcoin_wallet
                            .as_ref()
                            .context("Could not get Bitcoin wallet")?,
                    ),
                    (seed.clone(), context.config.namespace),
                );
                let mut swarm =
                    swarm::cli(seed.clone(), context.config.tor_socks5_port, behaviour).await?;
                let our_peer_id = swarm.local_peer_id();

                tracing::debug!(peer_id = %our_peer_id, "Network layer initialized");

                for seller_address in seller_addresses {
                    swarm
                        .behaviour_mut()
                        .add_address(seller_peer_id, seller_address);
                }

                let (event_loop, event_loop_handle) =
                    EventLoop::new(swap_id, swarm, seller_peer_id)?;
                let monero_receive_address = context.db.get_monero_address(swap_id).await?;
                let swap = Swap::from_db(
                    Arc::clone(&context.db),
                    swap_id,
                    Arc::clone(
                        context
                            .bitcoin_wallet
                            .as_ref()
                            .context("Could not get Bitcoin wallet")?,
                    ),
                    Arc::clone(
                        context
                            .monero_wallet
                            .as_ref()
                            .context("Could not get Monero wallet")?,
                    ),
                    context.config.env_config,
                    event_loop_handle,
                    monero_receive_address,
                )
                .await?;

                context.tasks.clone().spawn(
                    async move {
                        let handle = tokio::spawn(event_loop.run().in_current_span());
                        tokio::select! {
                            biased;
                            _ = context.swap_lock.listen_for_swap_force_suspension() => {
                                 tracing::debug!("Shutdown signal received, exiting");
                                context.swap_lock.release_swap_lock().await.expect("Shutdown signal received but failed to release swap lock. The swap process has been terminated but the swap lock is still active.");
                                bail!("Shutdown signal received");
                            },

                            event_loop_result = handle => {
                                match event_loop_result {
                                    Ok(_) => {
                                        tracing::debug!(%swap_id, "EventLoop completed during swap resume")
                                    }
                                    Err(error) => {
                                        tracing::error!(%swap_id, "EventLoop failed during swap resume: {:#}", error)
                                    }
                                }
                            },
                            swap_result = bob::run(swap) => {
                                match swap_result {
                                    Ok(state) => {
                                        tracing::debug!(%swap_id, state=%state, "Swap completed after resuming")
                                    }
                                    Err(error) => {
                                        tracing::error!(%swap_id, "Failed to resume swap: {:#}", error)
                                    }
                                }

                            }
                        }
                        context
                            .swap_lock
                            .release_swap_lock()
                            .await
                            .expect("Could not release swap lock");
                        Ok::<(), anyhow::Error>(())
                    }
                    .in_current_span(),
                ).await;
                Ok(json!({
                    "result": "ok",
                }))
            }
            Method::CancelAndRefund { swap_id } => {
                let bitcoin_wallet = context
                    .bitcoin_wallet
                    .as_ref()
                    .context("Could not get Bitcoin wallet")?;

                context.swap_lock.acquire_swap_lock(swap_id).await?;

                let state = cli::cancel_and_refund(
                    swap_id,
                    Arc::clone(bitcoin_wallet),
                    Arc::clone(&context.db),
                )
                .await;

                context
                    .swap_lock
                    .release_swap_lock()
                    .await
                    .expect("Could not release swap lock");

                state.map(|state| {
                    json!({
                        "result": state,
                    })
                })
            }
            Method::History => {
                let swaps = context.db.all().await?;
                let mut vec: Vec<(Uuid, String)> = Vec::new();
                for (swap_id, state) in swaps {
                    let state: BobState = state.try_into()?;
                    vec.push((swap_id, state.to_string()));
                }

                Ok(json!({ "swaps": vec }))
            }
            Method::GetRawStates => {
                let raw_history = context.db.raw_all().await?;

                Ok(json!({ "raw_states": raw_history }))
            }
            Method::Config => {
                let data_dir_display = context.config.data_dir.display();
                tracing::info!(path=%data_dir_display, "Data directory");
                tracing::info!(path=%format!("{}/logs", data_dir_display), "Log files directory");
                tracing::info!(path=%format!("{}/sqlite", data_dir_display), "Sqlite file location");
                tracing::info!(path=%format!("{}/seed.pem", data_dir_display), "Seed file location");
                tracing::info!(path=%format!("{}/monero", data_dir_display), "Monero-wallet-rpc directory");
                tracing::info!(path=%format!("{}/wallet", data_dir_display), "Internal bitcoin wallet directory");

                Ok(json!({
                    "log_files": format!("{}/logs", data_dir_display),
                    "sqlite": format!("{}/sqlite", data_dir_display),
                    "seed": format!("{}/seed.pem", data_dir_display),
                    "monero-wallet-rpc": format!("{}/monero", data_dir_display),
                    "bitcoin_wallet": format!("{}/wallet", data_dir_display),
                }))
            }
            Method::WithdrawBtc { address, amount } => {
                let bitcoin_wallet = context
                    .bitcoin_wallet
                    .as_ref()
                    .context("Could not get Bitcoin wallet")?;

                let amount = match amount {
                    Some(amount) => amount,
                    None => {
                        bitcoin_wallet
                            .max_giveable(address.script_pubkey().len())
                            .await?
                    }
                };
                let psbt = bitcoin_wallet
                    .send_to_address(address, amount, None)
                    .await?;
                let signed_tx = bitcoin_wallet.sign_and_finalize(psbt).await?;

                bitcoin_wallet
                    .broadcast(signed_tx.clone(), "withdraw")
                    .await?;

                Ok(json!({
                    "signed_tx": signed_tx,
                    "amount": amount.to_sat(),
                    "txid": signed_tx.txid(),
                }))
            }
            Method::StartDaemon { server_address } => {
                // Default to 127.0.0.1:1234
                let server_address = server_address.unwrap_or("127.0.0.1:1234".parse()?);

                let (addr, server_handle) =
                    rpc::run_server(server_address, Arc::clone(&context)).await?;

                tracing::info!(%addr, "Started RPC server");

                server_handle.stopped().await;

                tracing::info!("Stopped RPC server");

                Ok(json!({}))
            }
            Method::Balance { force_refresh } => {
                let bitcoin_wallet = context
                    .bitcoin_wallet
                    .as_ref()
                    .context("Could not get Bitcoin wallet")?;

                if force_refresh {
                    bitcoin_wallet.sync().await?;
                }

                let bitcoin_balance = bitcoin_wallet.balance().await?;

                if force_refresh {
                    tracing::info!(
                        balance = %bitcoin_balance,
                        "Checked Bitcoin balance",
                    );
                } else {
                    tracing::debug!(
                        balance = %bitcoin_balance,
                        "Current Bitcoin balance as of last sync",
                    );
                }

                Ok(json!({
                    "balance": bitcoin_balance.to_sat()
                }))
            }
            Method::ListSellers { rendezvous_point } => {
                let rendezvous_node_peer_id = rendezvous_point
                    .extract_peer_id()
                    .context("Rendezvous node address must contain peer ID")?;

                let identity = context
                    .config
                    .seed
                    .as_ref()
                    .context("Cannot extract seed")?
                    .derive_libp2p_identity();

                let sellers = list_sellers(
                    rendezvous_node_peer_id,
                    rendezvous_point,
                    context.config.namespace,
                    context.config.tor_socks5_port,
                    identity,
                )
                .await?;

                for seller in &sellers {
                    match seller.status {
                        SellerStatus::Online(quote) => {
                            tracing::info!(
                                price = %quote.price.to_string(),
                                min_quantity = %quote.min_quantity.to_string(),
                                max_quantity = %quote.max_quantity.to_string(),
                                status = "Online",
                                address = %seller.multiaddr.to_string(),
                                "Fetched peer status"
                            );
                        }
                        SellerStatus::Unreachable => {
                            tracing::info!(
                                status = "Unreachable",
                                address = %seller.multiaddr.to_string(),
                                "Fetched peer status"
                            );
                        }
                    }
                }

                Ok(json!({ "sellers": sellers }))
            }
            Method::ExportBitcoinWallet => {
                let bitcoin_wallet = context
                    .bitcoin_wallet
                    .as_ref()
                    .context("Could not get Bitcoin wallet")?;

                let wallet_export = bitcoin_wallet.wallet_export("cli").await?;
                tracing::info!(descriptor=%wallet_export.to_string(), "Exported bitcoin wallet");
                Ok(json!({
                    "descriptor": wallet_export.to_string(),
                }))
            }
            Method::MoneroRecovery { swap_id } => {
                let swap_state: BobState = context.db.get_state(swap_id).await?.try_into()?;

                if let BobState::BtcRedeemed(state5) = swap_state {
                    let (spend_key, view_key) = state5.xmr_keys();
                    let restore_height = state5.monero_wallet_restore_blockheight.height;

                    let address = monero::Address::standard(
                        context.config.env_config.monero_network,
                        monero::PublicKey::from_private_key(&spend_key),
                        monero::PublicKey::from(view_key.public()),
                    );

                    tracing::info!(restore_height=%restore_height, address=%address, spend_key=%spend_key, view_key=%view_key, "Monero recovery information");

                    Ok(json!({
                        "address": address,
                        "spend_key": spend_key.to_string(),
                        "view_key": view_key.to_string(),
                        "restore_height": state5.monero_wallet_restore_blockheight.height,
                    }))
                } else {
                    bail!(
                        "Cannot print monero recovery information in state {}, only possible for BtcRedeemed",
                        swap_state
                    )
                }
            }
            Method::GetCurrentSwap => Ok(json!({
                "swap_id": context.swap_lock.get_current_swap_id().await
            })),
            Method::AttemptCooperativeRedeem { swap_id } => {
                context.swap_lock.acquire_swap_lock(swap_id).await?;

                let seller_peer_id = context.db.get_peer_id(swap_id).await?;
                let seller_addresses = context.db.get_addresses(seller_peer_id).await?;
                let monero_wallet = Arc::clone(
                    context
                        .monero_wallet
                        .as_ref()
                        .context("Could not get Monero wallet")?,
                );
                let monero_receive_address = context.db.get_monero_address(swap_id).await?;
                let (state3, monero_wallet_restore_blockheight) = context
                    .db
                    .get_states(swap_id)
                    .await?
                    .iter()
                    .find_map(|state| {
                        if let State::Bob(BobState::BtcLocked {
                            state3,
                            monero_wallet_restore_blockheight,
                        }) = state
                        {
                            Some((state3.clone(), monero_wallet_restore_blockheight.clone()))
                        } else {
                            None
                        }
                    })
                    .expect("Could not get state3 from database");
                let seed = context
                    .config
                    .seed
                    .as_ref()
                    .context("Could not get seed")?
                    .derive_libp2p_identity();

                let behaviour = cli::Behaviour::new(
                    seller_peer_id,
                    context.config.env_config,
                    Arc::clone(
                        context
                            .bitcoin_wallet
                            .as_ref()
                            .context("Could not get Bitcoin wallet")?,
                    ),
                    (seed.clone(), context.config.namespace),
                );
                let mut swarm =
                    swarm::cli(seed.clone(), context.config.tor_socks5_port, behaviour).await?;
                let our_peer_id = swarm.local_peer_id();

                tracing::debug!(peer_id = %our_peer_id, "Network layer initialized");

                for seller_address in seller_addresses {
                    swarm
                        .behaviour_mut()
                        .add_address(seller_peer_id, seller_address);
                }

                let (_, mut event_loop_handle) = EventLoop::new(swap_id, swarm, seller_peer_id)?;
                tracing::info!("Asking Alice to reveal XMR key to us.");
                let response = event_loop_handle
                    .request_cooperative_xmr_redeem(swap_id)
                    .await;
                match response {
                    Ok(response) => {
                        tracing::info!("Alice revealed XMR key to us, redeeming XMR.");
                        let s_a = monero::PrivateKey {
                            scalar: response.s_a,
                        };
                        let state5 = state3
                            .attempt_cooperative_redeem(s_a, monero_wallet_restore_blockheight);
                        let (spend_key, view_key) = state5.xmr_keys();

                        let wallet_file_name = swap_id.to_string();

                        tracing::info!(%wallet_file_name, "Generating and opening Monero wallet from the extracted keys to redeem the Monero");
                        if let Err(e) = monero_wallet
                            .create_from_and_load(
                                wallet_file_name.clone(),
                                spend_key,
                                view_key,
                                state5.monero_wallet_restore_blockheight,
                            )
                            .await
                        {
                            // In case we failed to refresh/sweep, when resuming the wallet might already
                            // exist! This is a very unlikely scenario, but if we don't take care of it we
                            // might not be able to ever transfer the Monero.
                            tracing::warn!("Failed to generate monero wallet from keys: {:#}", e);
                            tracing::info!(%wallet_file_name,
                                "Falling back to trying to open the wallet if it already exists",
                            );
                            monero_wallet.open(wallet_file_name).await?;
                        }

                        // Ensure that the generated wallet is synced so we have a proper balance
                        monero_wallet.refresh().await?;
                        // Sweep (transfer all funds) to the given address
                        match monero_wallet.sweep_all(monero_receive_address).await {
                            Ok(tx_hashes) => {
                                for tx_hash in tx_hashes {
                                    tracing::info!(%monero_receive_address, txid=%tx_hash.0, "Successfully transferred XMR to wallet");
                                }
                                let state = BobState::XmrRedeemed {
                                    tx_lock_id: state3.tx_lock.txid(),
                                };
                                context
                                    .db
                                    .insert_latest_state(swap_id, state.clone().into())
                                    .await?;
                            }
                            Err(error) => {
                                tracing::error!(%error, "Failed to redeem XMR with key from Alice.");
                            }
                        }
                    }
                    Err(error) => {
                        tracing::error!(%error, "Failed to get XMR key from Alice.");
                    }
                };
                Ok(json!({
                    "result": "ok",
                }))
            }
        }
    }

    pub async fn call(self, context: Arc<Context>) -> Result<serde_json::Value> {
        let method_span = self.cmd.get_tracing_span(self.log_reference.clone());

        self.handle_cmd(context)
            .instrument(method_span.clone())
            .await
            .map_err(|err| {
                method_span.in_scope(|| {
                    tracing::debug!(err = format!("{:?}", err), "API call resulted in an error");
                });
                err
            })
    }
}

fn qr_code(value: &impl ToString) -> Result<String> {
    let code = QrCode::new(value.to_string())?;
    let qr_code = code
        .render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Light)
        .light_color(unicode::Dense1x2::Dark)
        .build();
    Ok(qr_code)
}

pub async fn determine_btc_to_swap<FB, TB, FMG, TMG, FS, TS, FFE, TFE>(
    json: bool,
    bid_quote: BidQuote,
    get_new_address: impl Future<Output = Result<bitcoin::Address>>,
    balance: FB,
    max_giveable_fn: FMG,
    sync: FS,
    estimate_fee: FFE,
) -> Result<(Amount, Amount)>
where
    TB: Future<Output = Result<Amount>>,
    FB: Fn() -> TB,
    TMG: Future<Output = Result<Amount>>,
    FMG: Fn() -> TMG,
    TS: Future<Output = Result<()>>,
    FS: Fn() -> TS,
    FFE: Fn(Amount) -> TFE,
    TFE: Future<Output = Result<Amount>>,
{
    if bid_quote.max_quantity == Amount::ZERO {
        bail!(ZeroQuoteReceived)
    }

    tracing::info!(
        price = %bid_quote.price,
        minimum_amount = %bid_quote.min_quantity,
        maximum_amount = %bid_quote.max_quantity,
        "Received quote",
    );

    sync().await?;
    let mut max_giveable = max_giveable_fn().await?;

    if max_giveable == Amount::ZERO || max_giveable < bid_quote.min_quantity {
        let deposit_address = get_new_address.await?;
        let minimum_amount = bid_quote.min_quantity;
        let maximum_amount = bid_quote.max_quantity;

        if !json {
            eprintln!("{}", qr_code(&deposit_address)?);
        }

        loop {
            let min_outstanding = bid_quote.min_quantity - max_giveable;
            let min_bitcoin_lock_tx_fee = estimate_fee(min_outstanding).await?;
            let min_deposit_until_swap_will_start = min_outstanding + min_bitcoin_lock_tx_fee;
            let max_deposit_until_maximum_amount_is_reached =
                maximum_amount - max_giveable + min_bitcoin_lock_tx_fee;

            tracing::info!(
                "Deposit at least {} to cover the min quantity with fee!",
                min_deposit_until_swap_will_start
            );
            tracing::info!(
                %deposit_address,
                %min_deposit_until_swap_will_start,
                %max_deposit_until_maximum_amount_is_reached,
                %max_giveable,
                %minimum_amount,
                %maximum_amount,
                %min_bitcoin_lock_tx_fee,
                price = %bid_quote.price,
                "Waiting for Bitcoin deposit",
            );

            max_giveable = loop {
                sync().await?;
                let new_max_givable = max_giveable_fn().await?;

                if new_max_givable > max_giveable {
                    break new_max_givable;
                }

                tokio::time::sleep(Duration::from_secs(1)).await;
            };

            let new_balance = balance().await?;
            tracing::info!(%new_balance, %max_giveable, "Received Bitcoin");

            if max_giveable < bid_quote.min_quantity {
                tracing::info!("Deposited amount is not enough to cover `min_quantity` when accounting for network fees");
                continue;
            }

            break;
        }
    };

    let balance = balance().await?;
    let fees = balance - max_giveable;
    let max_accepted = bid_quote.max_quantity;
    let btc_swap_amount = min(max_giveable, max_accepted);

    Ok((btc_swap_amount, fees))
}
