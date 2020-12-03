use crate::{
    alice::{amounts, OutEvent, Swarm},
    bitcoin, monero,
    network::request_response::AliceToBob,
    SwapAmounts, PUNISH_TIMELOCK, REFUND_TIMELOCK,
};
use anyhow::{bail, Context, Result};
use conquer_once::Lazy;
use ecdsa_fun::{adaptor::Adaptor, nonce::Deterministic};
use futures::{
    future::{select, Either},
    pin_mut,
};
use libp2p::request_response::ResponseChannel;
use sha2::Sha256;
use std::{sync::Arc, time::Duration};
use tokio::time::timeout;
use tracing::trace;
use xmr_btc::{
    alice,
    alice::{State0, State3},
    bitcoin::{
        poll_until_block_height_is_gte, BlockHeight, BroadcastSignedTransaction,
        EncryptedSignature, GetRawTransaction, TransactionBlockHeight, TxCancel, TxLock, TxRefund,
        WaitForTransactionFinality, WatchForRawTransaction,
    },
    config::Config,
    cross_curve_dleq,
    monero::Transfer,
};

// The maximum we assume we need to wait from the moment the monero transaction
// is mined to the moment it reaches finality. We set 15 confirmations for now
// (based on Kraken). 1.5 multiplier in case the blockchain is slower than
// usually. Average of 2 minutes block time
static MONERO_MAX_FINALITY_TIME: Lazy<Duration> =
    Lazy::new(|| Duration::from_secs_f64(15f64 * 1.5 * 2f64 * 60f64));

pub async fn negotiate(
    amounts: SwapAmounts,
    a: bitcoin::SecretKey,
    s_a: cross_curve_dleq::Scalar,
    v_a: monero::PrivateViewKey,
    swarm: &mut Swarm,
    bitcoin_wallet: Arc<bitcoin::Wallet>,
    config: Config,
) -> Result<(ResponseChannel<AliceToBob>, State3)> {
    trace!("Starting negotiate");
    let event = timeout(config.bob_time_to_act, swarm.next())
        .await
        .context("Failed to receive dial connection from Bob")?;
    match event {
        OutEvent::ConnectionEstablished(_bob_peer_id) => {}
        other => bail!("Unexpected event received: {:?}", other),
    }

    let event = timeout(config.bob_time_to_act, swarm.next())
        .await
        .context("Failed to receive amounts from Bob")?;
    let (btc, channel) = match event {
        OutEvent::Request(amounts::OutEvent::Btc { btc, channel }) => (btc, channel),
        other => bail!("Unexpected event received: {:?}", other),
    };

    if btc != amounts.btc {
        bail!(
            "Bob proposed a different amount; got {}, expected: {}",
            btc,
            amounts.btc
        );
    }
    // TODO: get an ack from libp2p2
    swarm.send_amounts(channel, amounts);

    let redeem_address = bitcoin_wallet.as_ref().new_address().await?;
    let punish_address = redeem_address.clone();

    let state0 = State0::new(
        a,
        s_a,
        v_a,
        amounts.btc,
        amounts.xmr,
        REFUND_TIMELOCK,
        PUNISH_TIMELOCK,
        redeem_address,
        punish_address,
    );

    // TODO(Franck): Understand why this is needed.
    swarm.set_state0(state0.clone());

    let event = timeout(config.bob_time_to_act, swarm.next())
        .await
        .context("Failed to receive message 0 from Bob")?;
    let message0 = match event {
        OutEvent::Message0(msg) => msg,
        other => bail!("Unexpected event received: {:?}", other),
    };

    let state1 = state0.receive(message0)?;

    let event = timeout(config.bob_time_to_act, swarm.next())
        .await
        .context("Failed to receive message 1 from Bob")?;
    let (msg, channel) = match event {
        OutEvent::Message1 { msg, channel } => (msg, channel),
        other => bail!("Unexpected event: {:?}", other),
    };

    let state2 = state1.receive(msg);

    let message1 = state2.next_message();
    swarm.send_message1(channel, message1);

    let event = timeout(config.bob_time_to_act, swarm.next())
        .await
        .context("Failed to receive message 2 from Bob")?;
    let (msg, channel) = match event {
        OutEvent::Message2 { msg, channel } => (msg, channel),
        other => bail!("Unexpected event: {:?}", other),
    };

    let state3 = state2.receive(msg)?;

    Ok((channel, state3))
}

pub async fn wait_for_locked_bitcoin<W>(
    lock_bitcoin_txid: bitcoin::Txid,
    bitcoin_wallet: Arc<W>,
    config: Config,
) -> Result<()>
where
    W: WatchForRawTransaction + WaitForTransactionFinality,
{
    // We assume we will see Bob's transaction in the mempool first.
    timeout(
        config.bob_time_to_act,
        bitcoin_wallet.watch_for_raw_transaction(lock_bitcoin_txid),
    )
    .await
    .context("Failed to find lock Bitcoin tx")?;

    // // We saw the transaction in the mempool, waiting for it to be confirmed.
    // bitcoin_wallet
    //     .wait_for_transaction_finality(lock_bitcoin_txid, config)
    //     .await;

    Ok(())
}

pub async fn lock_xmr<W>(
    channel: ResponseChannel<AliceToBob>,
    amounts: SwapAmounts,
    state3: State3,
    swarm: &mut Swarm,
    monero_wallet: Arc<W>,
) -> Result<()>
where
    W: Transfer,
{
    let S_a = monero::PublicKey::from_private_key(&monero::PrivateKey {
        scalar: state3.s_a.into_ed25519(),
    });

    let public_spend_key = S_a + state3.S_b_monero;
    let public_view_key = state3.v.public();

    let (transfer_proof, _) = monero_wallet
        .transfer(public_spend_key, public_view_key, amounts.xmr)
        .await?;

    // TODO(Franck): Wait for Monero to be confirmed once

    swarm.send_message2(channel, alice::Message2 {
        tx_lock_proof: transfer_proof,
    });

    Ok(())
}

pub async fn wait_for_bitcoin_encrypted_signature(swarm: &mut Swarm) -> Result<EncryptedSignature> {
    let event = timeout(*MONERO_MAX_FINALITY_TIME, swarm.next())
        .await
        .context("Failed to receive Bitcoin encrypted signature from Bob")?;

    match event {
        OutEvent::Message3(msg) => Ok(msg.tx_redeem_encsig),
        other => bail!(
            "Expected Bob's Bitcoin redeem encrypted signature, got: {:?}",
            other
        ),
    }
}

pub fn build_bitcoin_redeem_transaction(
    encrypted_signature: EncryptedSignature,
    tx_lock: &TxLock,
    a: bitcoin::SecretKey,
    s_a: cross_curve_dleq::Scalar,
    B: bitcoin::PublicKey,
    redeem_address: &bitcoin::Address,
) -> Result<bitcoin::Transaction> {
    let adaptor = Adaptor::<Sha256, Deterministic<Sha256>>::default();

    let tx_redeem = bitcoin::TxRedeem::new(tx_lock, redeem_address);

    bitcoin::verify_encsig(
        B,
        s_a.into_secp256k1().into(),
        &tx_redeem.digest(),
        &encrypted_signature,
    )
    .context("Invalid encrypted signature received")?;

    let sig_a = a.sign(tx_redeem.digest());
    let sig_b = adaptor.decrypt_signature(&s_a.into_secp256k1(), encrypted_signature);

    let tx = tx_redeem
        .add_signatures(&tx_lock, (a.public(), sig_a), (B, sig_b))
        .context("sig_{a,b} are invalid for tx_redeem")?;

    Ok(tx)
}

pub async fn publish_bitcoin_redeem_transaction<W>(
    redeem_tx: bitcoin::Transaction,
    bitcoin_wallet: Arc<W>,
    config: Config,
) -> Result<()>
where
    W: BroadcastSignedTransaction + WaitForTransactionFinality,
{
    let tx_id = bitcoin_wallet
        .broadcast_signed_transaction(redeem_tx)
        .await?;

    bitcoin_wallet
        .wait_for_transaction_finality(tx_id, config)
        .await
}

pub async fn publish_cancel_transaction<W>(
    tx_lock: TxLock,
    a: bitcoin::SecretKey,
    B: bitcoin::PublicKey,
    refund_timelock: u32,
    tx_cancel_sig_bob: bitcoin::Signature,
    bitcoin_wallet: Arc<W>,
) -> Result<bitcoin::TxCancel>
where
    W: GetRawTransaction + TransactionBlockHeight + BlockHeight + BroadcastSignedTransaction,
{
    // First wait for t1 to expire
    let tx_lock_height = bitcoin_wallet
        .transaction_block_height(tx_lock.txid())
        .await;
    poll_until_block_height_is_gte(bitcoin_wallet.as_ref(), tx_lock_height + refund_timelock).await;

    let tx_cancel = bitcoin::TxCancel::new(&tx_lock, refund_timelock, a.public(), B);

    // If Bob hasn't yet broadcasted the tx cancel, we do it
    if bitcoin_wallet
        .get_raw_transaction(tx_cancel.txid())
        .await
        .is_err()
    {
        // TODO(Franck): Maybe the cancel transaction is already mined, in this case,
        // the broadcast will error out.

        let sig_a = a.sign(tx_cancel.digest());
        let sig_b = tx_cancel_sig_bob.clone();

        let tx_cancel = tx_cancel
            .clone()
            .add_signatures(&tx_lock, (a.public(), sig_a), (B, sig_b))
            .expect("sig_{a,b} to be valid signatures for tx_cancel");

        // TODO(Franck): Error handling is delicate, why can't we broadcast?
        bitcoin_wallet
            .broadcast_signed_transaction(tx_cancel)
            .await?;

        // TODO(Franck): Wait until transaction is mined and returned mined
        // block height
    }

    Ok(tx_cancel)
}

pub async fn wait_for_bitcoin_refund<W>(
    tx_cancel: &TxCancel,
    cancel_tx_height: u32,
    punish_timelock: u32,
    refund_address: &bitcoin::Address,
    bitcoin_wallet: Arc<W>,
) -> Result<(bitcoin::TxRefund, Option<bitcoin::Transaction>)>
where
    W: BlockHeight + WatchForRawTransaction,
{
    let punish_timelock_expired =
        poll_until_block_height_is_gte(bitcoin_wallet.as_ref(), cancel_tx_height + punish_timelock);

    let tx_refund = bitcoin::TxRefund::new(tx_cancel, refund_address);

    // TODO(Franck): This only checks the mempool, need to cater for the case where
    // the transaction goes directly in a block
    let seen_refund_tx = bitcoin_wallet.watch_for_raw_transaction(tx_refund.txid());

    pin_mut!(punish_timelock_expired);
    pin_mut!(seen_refund_tx);

    match select(punish_timelock_expired, seen_refund_tx).await {
        Either::Left(_) => Ok((tx_refund, None)),
        Either::Right((published_refund_tx, _)) => Ok((tx_refund, Some(published_refund_tx))),
    }
}

pub fn extract_monero_private_key(
    published_refund_tx: bitcoin::Transaction,
    tx_refund: TxRefund,
    s_a: cross_curve_dleq::Scalar,
    a: bitcoin::SecretKey,
    S_b_bitcoin: bitcoin::PublicKey,
) -> Result<monero::PrivateKey> {
    let s_a = monero::PrivateKey {
        scalar: s_a.into_ed25519(),
    };

    let tx_refund_sig = tx_refund
        .extract_signature_by_key(published_refund_tx, a.public())
        .context("Failed to extract signature from Bitcoin refund tx")?;
    let tx_refund_encsig = a.encsign(S_b_bitcoin, tx_refund.digest());

    let s_b = bitcoin::recover(S_b_bitcoin, tx_refund_sig, tx_refund_encsig)
        .context("Failed to recover Monero secret key from Bitcoin signature")?;
    let s_b = monero::private_key_from_secp256k1_scalar(s_b.into());

    let spend_key = s_a + s_b;

    Ok(spend_key)
}

pub fn build_bitcoin_punish_transaction(
    tx_lock: &TxLock,
    refund_timelock: u32,
    punish_address: &bitcoin::Address,
    punish_timelock: u32,
    tx_punish_sig_bob: bitcoin::Signature,
    a: bitcoin::SecretKey,
    B: bitcoin::PublicKey,
) -> Result<bitcoin::Transaction> {
    let tx_cancel = bitcoin::TxCancel::new(&tx_lock, refund_timelock, a.public(), B);
    let tx_punish = bitcoin::TxPunish::new(&tx_cancel, &punish_address, punish_timelock);

    let sig_a = a.sign(tx_punish.digest());
    let sig_b = tx_punish_sig_bob;

    let signed_tx_punish = tx_punish
        .add_signatures(&tx_cancel, (a.public(), sig_a), (B, sig_b))
        .expect("sig_{a,b} to be valid signatures for tx_cancel");

    Ok(signed_tx_punish)
}

pub async fn publish_bitcoin_punish_transaction<W>(
    punish_tx: bitcoin::Transaction,
    bitcoin_wallet: Arc<W>,
    config: Config,
) -> Result<bitcoin::Txid>
where
    W: BroadcastSignedTransaction + WaitForTransactionFinality,
{
    let txid = bitcoin_wallet
        .broadcast_signed_transaction(punish_tx)
        .await?;

    bitcoin_wallet
        .wait_for_transaction_finality(txid, config)
        .await?;

    Ok(txid)
}
