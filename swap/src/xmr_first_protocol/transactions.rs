pub mod btc_lock;
// pub mod btc_redeem;
pub mod xmr_lock;
pub mod xmr_refund;

use crate::bitcoin::wallet::Watchable;
use crate::bitcoin::{
    build_shared_output_descriptor, Address, Amount, PartiallySignedTransaction, PublicKey,
    Transaction, Txid, Wallet, TX_FEE,
};
use anyhow::{bail, Result};
use bdk::bitcoin::{OutPoint, Script, TxIn, TxOut};
use bdk::database::BatchDatabase;
use bdk::descriptor::Descriptor;
use ecdsa_fun::fun::Point;
use miniscript::DescriptorTrait;
use rand::thread_rng;
