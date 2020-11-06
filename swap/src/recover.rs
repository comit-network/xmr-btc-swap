//! This module is used to attempt to recover an unfinished swap.
//!
//! Recovery is only supported for certain states and the strategy followed is
//! to perform the simplest steps that require no further action from the
//! counterparty.
//!
//! The quality of this module is bad because there is a lot of code
//! duplication, both within the module and with respect to
//! `xmr_btc/src/{alice,bob}.rs`. In my opinion, a better approach to support
//! swap recovery would be through the `action_generator`s themselves, but this
//! was deemed too complicated for the time being.

use crate::{
    bitcoin, monero,
    monero::CreateWalletForOutput,
    state::{Alice, Bob, Swap},
};
use anyhow::Result;
use ecdsa_fun::{adaptor::Adaptor, nonce::Deterministic};
use futures::{
    future::{select, Either},
    pin_mut,
};
use sha2::Sha256;
use tracing::info;
use xmr_btc::bitcoin::{
    poll_until_block_height_is_gte, BroadcastSignedTransaction, TransactionBlockHeight,
    WatchForRawTransaction,
};

pub async fn recover(
    bitcoin_wallet: bitcoin::Wallet,
    monero_wallet: monero::Wallet,
    state: Swap,
) -> Result<()> {
    match state {
        Swap::Alice(state) => alice_recover(bitcoin_wallet, monero_wallet, state).await,
        Swap::Bob(state) => bob_recover(bitcoin_wallet, monero_wallet, state).await,
    }
}

pub async fn alice_recover(
    bitcoin_wallet: bitcoin::Wallet,
    monero_wallet: monero::Wallet,
    state: Alice,
) -> Result<()> {
    match state {
        Alice::Handshaken(_) | Alice::BtcLocked(_) | Alice::SwapComplete => {
            info!("Nothing to do");
        }
        Alice::XmrLocked(state) => {
            info!("Monero still locked up");

            let tx_cancel = bitcoin::TxCancel::new(
                &state.tx_lock,
                state.refund_timelock,
                state.a.public(),
                state.B.clone(),
            );

            info!("Checking if the Bitcoin cancel transaction has been published");
            if bitcoin_wallet
                .0
                .get_raw_transaction(tx_cancel.txid())
                .await
                .is_err()
            {
                info!("Bitcoin cancel transaction not yet published");

                let tx_lock_height = bitcoin_wallet
                    .transaction_block_height(state.tx_lock.txid())
                    .await;
                poll_until_block_height_is_gte(
                    &bitcoin_wallet,
                    tx_lock_height + state.refund_timelock,
                )
                .await;

                let sig_a = state.a.sign(tx_cancel.digest());
                let sig_b = state.tx_cancel_sig_bob.clone();

                let tx_cancel = tx_cancel
                    .clone()
                    .add_signatures(
                        &state.tx_lock,
                        (state.a.public(), sig_a),
                        (state.B.clone(), sig_b),
                    )
                    .expect("sig_{a,b} to be valid signatures for tx_cancel");

                // TODO: We should not fail if the transaction is already on the blockchain
                bitcoin_wallet
                    .broadcast_signed_transaction(tx_cancel)
                    .await?;
            }

            info!("Confirmed that Bitcoin cancel transaction is on the blockchain");

            let tx_cancel_height = bitcoin_wallet
                .transaction_block_height(tx_cancel.txid())
                .await;
            let poll_until_bob_can_be_punished = poll_until_block_height_is_gte(
                &bitcoin_wallet,
                tx_cancel_height + state.punish_timelock,
            );
            pin_mut!(poll_until_bob_can_be_punished);

            let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &state.refund_address);

            info!("Waiting for either Bitcoin refund or punish timelock");
            match select(
                bitcoin_wallet.watch_for_raw_transaction(tx_refund.txid()),
                poll_until_bob_can_be_punished,
            )
            .await
            {
                Either::Left((tx_refund_published, ..)) => {
                    info!("Found Bitcoin refund transaction");

                    let s_a = monero::PrivateKey {
                        scalar: state.s_a.into_ed25519(),
                    };

                    let tx_refund_sig = tx_refund
                        .extract_signature_by_key(tx_refund_published, state.a.public())?;
                    let tx_refund_encsig = state
                        .a
                        .encsign(state.S_b_bitcoin.clone(), tx_refund.digest());

                    let s_b = bitcoin::recover(state.S_b_bitcoin, tx_refund_sig, tx_refund_encsig)?;
                    let s_b = monero::PrivateKey::from_scalar(
                        monero::Scalar::from_bytes_mod_order(s_b.to_bytes()),
                    );

                    monero_wallet
                        .create_and_load_wallet_for_output(s_a + s_b, state.v)
                        .await?;
                    info!("Successfully refunded monero");
                }
                Either::Right(_) => {
                    info!("Punish timelock reached, attempting to punish Bob");

                    let tx_punish = bitcoin::TxPunish::new(
                        &tx_cancel,
                        &state.punish_address,
                        state.punish_timelock,
                    );

                    let sig_a = state.a.sign(tx_punish.digest());
                    let sig_b = state.tx_punish_sig_bob.clone();

                    let sig_tx_punish = tx_punish.add_signatures(
                        &tx_cancel,
                        (state.a.public(), sig_a),
                        (state.B.clone(), sig_b),
                    )?;

                    bitcoin_wallet
                        .broadcast_signed_transaction(sig_tx_punish)
                        .await?;
                    info!("Successfully punished Bob's inactivity by taking bitcoin");
                }
            };
        }
        Alice::BtcRedeemable { redeem_tx, state } => {
            info!("Have the means to redeem the Bitcoin");

            let tx_lock_height = bitcoin_wallet
                .transaction_block_height(state.tx_lock.txid())
                .await;

            let block_height = bitcoin_wallet.0.block_height().await?;
            let refund_absolute_expiry = tx_lock_height + state.refund_timelock;

            info!("Checking refund timelock");
            if block_height < refund_absolute_expiry {
                info!("Safe to redeem");

                bitcoin_wallet
                    .broadcast_signed_transaction(redeem_tx)
                    .await?;
                info!("Successfully redeemed bitcoin");
            } else {
                info!("Refund timelock reached");

                let tx_cancel = bitcoin::TxCancel::new(
                    &state.tx_lock,
                    state.refund_timelock,
                    state.a.public(),
                    state.B.clone(),
                );

                info!("Checking if the Bitcoin cancel transaction has been published");
                if bitcoin_wallet
                    .0
                    .get_raw_transaction(tx_cancel.txid())
                    .await
                    .is_err()
                {
                    let sig_a = state.a.sign(tx_cancel.digest());
                    let sig_b = state.tx_cancel_sig_bob.clone();

                    let tx_cancel = tx_cancel
                        .clone()
                        .add_signatures(
                            &state.tx_lock,
                            (state.a.public(), sig_a),
                            (state.B.clone(), sig_b),
                        )
                        .expect("sig_{a,b} to be valid signatures for tx_cancel");

                    // TODO: We should not fail if the transaction is already on the blockchain
                    bitcoin_wallet
                        .broadcast_signed_transaction(tx_cancel)
                        .await?;
                }

                info!("Confirmed that Bitcoin cancel transaction is on the blockchain");

                let tx_cancel_height = bitcoin_wallet
                    .transaction_block_height(tx_cancel.txid())
                    .await;
                let poll_until_bob_can_be_punished = poll_until_block_height_is_gte(
                    &bitcoin_wallet,
                    tx_cancel_height + state.punish_timelock,
                );
                pin_mut!(poll_until_bob_can_be_punished);

                let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &state.refund_address);

                info!("Waiting for either Bitcoin refund or punish timelock");
                match select(
                    bitcoin_wallet.watch_for_raw_transaction(tx_refund.txid()),
                    poll_until_bob_can_be_punished,
                )
                .await
                {
                    Either::Left((tx_refund_published, ..)) => {
                        info!("Found Bitcoin refund transaction");

                        let s_a = monero::PrivateKey {
                            scalar: state.s_a.into_ed25519(),
                        };

                        let tx_refund_sig = tx_refund
                            .extract_signature_by_key(tx_refund_published, state.a.public())?;
                        let tx_refund_encsig = state
                            .a
                            .encsign(state.S_b_bitcoin.clone(), tx_refund.digest());

                        let s_b =
                            bitcoin::recover(state.S_b_bitcoin, tx_refund_sig, tx_refund_encsig)?;
                        let s_b = monero::PrivateKey::from_scalar(
                            monero::Scalar::from_bytes_mod_order(s_b.to_bytes()),
                        );

                        monero_wallet
                            .create_and_load_wallet_for_output(s_a + s_b, state.v)
                            .await?;
                        info!("Successfully refunded monero");
                    }
                    Either::Right(_) => {
                        info!("Punish timelock reached, attempting to punish Bob");

                        let tx_punish = bitcoin::TxPunish::new(
                            &tx_cancel,
                            &state.punish_address,
                            state.punish_timelock,
                        );

                        let sig_a = state.a.sign(tx_punish.digest());
                        let sig_b = state.tx_punish_sig_bob.clone();

                        let sig_tx_punish = tx_punish.add_signatures(
                            &tx_cancel,
                            (state.a.public(), sig_a),
                            (state.B.clone(), sig_b),
                        )?;

                        bitcoin_wallet
                            .broadcast_signed_transaction(sig_tx_punish)
                            .await?;
                        info!("Successfully punished Bob's inactivity by taking bitcoin");
                    }
                };
            }
        }
        Alice::BtcPunishable(state) => {
            info!("Punish timelock reached, attempting to punish Bob");

            let tx_cancel = bitcoin::TxCancel::new(
                &state.tx_lock,
                state.refund_timelock,
                state.a.public(),
                state.B.clone(),
            );
            let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &state.refund_address);

            info!("Checking if Bitcoin has already been refunded");

            // TODO: Protect against transient errors so that we can correctly decide if the
            // bitcoin has been refunded
            match bitcoin_wallet.0.get_raw_transaction(tx_refund.txid()).await {
                Ok(tx_refund_published) => {
                    info!("Bitcoin already refunded");

                    let s_a = monero::PrivateKey {
                        scalar: state.s_a.into_ed25519(),
                    };

                    let tx_refund_sig = tx_refund
                        .extract_signature_by_key(tx_refund_published, state.a.public())?;
                    let tx_refund_encsig = state
                        .a
                        .encsign(state.S_b_bitcoin.clone(), tx_refund.digest());

                    let s_b = bitcoin::recover(state.S_b_bitcoin, tx_refund_sig, tx_refund_encsig)?;
                    let s_b = monero::PrivateKey::from_scalar(
                        monero::Scalar::from_bytes_mod_order(s_b.to_bytes()),
                    );

                    monero_wallet
                        .create_and_load_wallet_for_output(s_a + s_b, state.v)
                        .await?;
                    info!("Successfully refunded monero");
                }
                Err(_) => {
                    info!("Bitcoin not yet refunded");

                    let tx_punish = bitcoin::TxPunish::new(
                        &tx_cancel,
                        &state.punish_address,
                        state.punish_timelock,
                    );

                    let sig_a = state.a.sign(tx_punish.digest());
                    let sig_b = state.tx_punish_sig_bob.clone();

                    let sig_tx_punish = tx_punish.add_signatures(
                        &tx_cancel,
                        (state.a.public(), sig_a),
                        (state.B.clone(), sig_b),
                    )?;

                    bitcoin_wallet
                        .broadcast_signed_transaction(sig_tx_punish)
                        .await?;
                    info!("Successfully punished Bob's inactivity by taking bitcoin");
                }
            }
        }
        Alice::BtcRefunded {
            view_key,
            spend_key,
            ..
        } => {
            info!("Bitcoin was refunded, attempting to refund monero");

            monero_wallet
                .create_and_load_wallet_for_output(spend_key, view_key)
                .await?;
            info!("Successfully refunded monero");
        }
    };

    Ok(())
}

pub async fn bob_recover(
    bitcoin_wallet: crate::bitcoin::Wallet,
    monero_wallet: crate::monero::Wallet,
    state: Bob,
) -> Result<()> {
    match state {
        Bob::Handshaken(_) | Bob::SwapComplete => {
            info!("Nothing to do");
        }
        Bob::BtcLocked(state) | Bob::XmrLocked(state) | Bob::BtcRefundable(state) => {
            info!("Bitcoin may still be locked up, attempting to refund");

            let tx_cancel = bitcoin::TxCancel::new(
                &state.tx_lock,
                state.refund_timelock,
                state.A.clone(),
                state.b.public(),
            );

            info!("Checking if the Bitcoin cancel transaction has been published");
            if bitcoin_wallet
                .0
                .get_raw_transaction(tx_cancel.txid())
                .await
                .is_err()
            {
                info!("Bitcoin cancel transaction not yet published");

                let tx_lock_height = bitcoin_wallet
                    .transaction_block_height(state.tx_lock.txid())
                    .await;
                poll_until_block_height_is_gte(
                    &bitcoin_wallet,
                    tx_lock_height + state.refund_timelock,
                )
                .await;

                let sig_a = state.tx_cancel_sig_a.clone();
                let sig_b = state.b.sign(tx_cancel.digest());

                let tx_cancel = tx_cancel
                    .clone()
                    .add_signatures(
                        &state.tx_lock,
                        (state.A.clone(), sig_a),
                        (state.b.public(), sig_b),
                    )
                    .expect("sig_{a,b} to be valid signatures for tx_cancel");

                // TODO: We should not fail if the transaction is already on the blockchain
                bitcoin_wallet
                    .broadcast_signed_transaction(tx_cancel)
                    .await?;
            }

            info!("Confirmed that Bitcoin cancel transaction is on the blockchain");

            let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &state.refund_address);
            let signed_tx_refund = {
                let adaptor = Adaptor::<Sha256, Deterministic<Sha256>>::default();
                let sig_a = adaptor
                    .decrypt_signature(&state.s_b.into_secp256k1(), state.tx_refund_encsig.clone());
                let sig_b = state.b.sign(tx_refund.digest());

                tx_refund
                    .add_signatures(
                        &tx_cancel,
                        (state.A.clone(), sig_a),
                        (state.b.public(), sig_b),
                    )
                    .expect("sig_{a,b} to be valid signatures for tx_refund")
            };

            // TODO: Check if Bitcoin has already been punished and provide a useful error
            // message/log to the user if so
            bitcoin_wallet
                .broadcast_signed_transaction(signed_tx_refund)
                .await?;
            info!("Successfully refunded bitcoin");
        }
        Bob::BtcRedeemed(state) => {
            info!("Bitcoin was redeemed, attempting to redeem monero");

            let tx_redeem = bitcoin::TxRedeem::new(&state.tx_lock, &state.redeem_address);
            let tx_redeem_published = bitcoin_wallet
                .0
                .get_raw_transaction(tx_redeem.txid())
                .await?;

            let tx_redeem_encsig = state
                .b
                .encsign(state.S_a_bitcoin.clone(), tx_redeem.digest());
            let tx_redeem_sig =
                tx_redeem.extract_signature_by_key(tx_redeem_published, state.b.public())?;

            let s_a = bitcoin::recover(state.S_a_bitcoin, tx_redeem_sig, tx_redeem_encsig)?;
            let s_a = monero::PrivateKey::from_scalar(monero::Scalar::from_bytes_mod_order(
                s_a.to_bytes(),
            ));

            let s_b = monero::PrivateKey {
                scalar: state.s_b.into_ed25519(),
            };

            monero_wallet
                .create_and_load_wallet_for_output(s_a + s_b, state.v)
                .await?;
            info!("Successfully redeemed monero")
        }
    };

    Ok(())
}
