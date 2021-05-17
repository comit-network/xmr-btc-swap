mod v2;

use anyhow::{Context, Result};
use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
use curve25519_dalek::edwards::{CompressedEdwardsY, EdwardsPoint};
use curve25519_dalek::scalar::Scalar;
use hash_edwards_to_edwards::hash_point_to_point;
use itertools::Itertools;
use monero::blockdata::transaction::{ExtraField, KeyImage, SubField, TxOutTarget};
use monero::cryptonote::hash::Hashable;
use monero::cryptonote::onetime_key::KeyGenerator;
use monero::util::key::H;
use monero::util::ringct::{CtKey, EcdhInfo, Key, RctSig, RctSigBase, RctSigPrunable, RctType};
use monero::{
    Address, KeyPair, OwnedTxOut, PrivateKey, PublicKey, Transaction, TransactionPrefix, TxIn,
    TxOut, VarInt,
};
use monero_rpc::monerod;
use monero_rpc::monerod::{GetBlockResponse, GetOutputsOut, MonerodRpc as _};
use rand::{CryptoRng, RngCore};
use std::convert::TryInto;
use std::iter;

pub struct ConfidentialTransactionBuilder {
    outputs: Vec<TxOut>,
    ecdh_info: Vec<EcdhInfo>,
    extra: ExtraField,

    blinding_factors: Vec<Scalar>,
    amounts: Vec<u64>,

    decoy_inputs: [DecoyInput; 10],

    actual_signing_key: Scalar,
    real_commitment_blinder: Scalar,
    signing_pk: EdwardsPoint,
    H_p_pk: EdwardsPoint,
    input_commitment: EdwardsPoint,
    spend_amount: u64,
    global_output_index: u64,
}

impl ConfidentialTransactionBuilder {
    pub fn new(
        input_to_spend: OwnedTxOut<'_>,
        global_output_index: u64,
        decoy_inputs: [DecoyInput; 10],
        keys: KeyPair,
    ) -> Self {
        let actual_signing_key = input_to_spend.recover_key(&keys).scalar;
        let signing_pk = actual_signing_key * ED25519_BASEPOINT_POINT;

        Self {
            outputs: vec![],
            ecdh_info: vec![],
            extra: Default::default(),
            blinding_factors: vec![],
            amounts: vec![],
            decoy_inputs,
            actual_signing_key,
            signing_pk,
            H_p_pk: hash_point_to_point(signing_pk),
            input_commitment: input_to_spend.commitment().unwrap(), // TODO: Error handling
            spend_amount: input_to_spend.amount().unwrap(),         // TODO: Error handling,
            global_output_index,
            real_commitment_blinder: input_to_spend.blinding_factor().unwrap(), // TODO: Error handling
        }
    }

    pub fn with_output(
        mut self,
        to: Address,
        amount: u64,
        rng: &mut (impl RngCore + CryptoRng),
    ) -> Self {
        let next_index = self.outputs.len();

        let ecdh_key = PrivateKey::random(rng);
        let (ecdh_info, blinding_factor) = EcdhInfo::new_bulletproof(amount, ecdh_key.scalar);

        let out = TxOut {
            amount: VarInt(0),
            target: TxOutTarget::ToKey {
                key: KeyGenerator::from_random(to.public_view, to.public_spend, ecdh_key)
                    .one_time_key(dbg!(next_index)),
            },
        };

        self.outputs.push(out);
        self.extra
            .0
            .push(SubField::TxPublicKey(PublicKey::from_private_key(
                &ecdh_key,
            )));
        self.ecdh_info.push(ecdh_info);
        self.blinding_factors.push(blinding_factor);
        self.amounts.push(amount);

        // sanity checks
        debug_assert_eq!(self.outputs.len(), self.extra.0.len());
        debug_assert_eq!(self.outputs.len(), self.blinding_factors.len());
        debug_assert_eq!(self.outputs.len(), self.amounts.len());

        self
    }

    fn compute_fee(&self) -> u64 {
        self.spend_amount - self.amounts.iter().sum::<u64>()
    }

    fn compute_pseudo_out(&mut self, commitments: &[EdwardsPoint]) -> EdwardsPoint {
        let sum_commitments = commitments.iter().sum::<EdwardsPoint>();

        let fee = self.compute_fee();

        let fee_key = Scalar::from(fee) * H.point.decompress().unwrap();

        fee_key + sum_commitments
    }

    fn compute_key_image(&self) -> EdwardsPoint {
        self.actual_signing_key * self.H_p_pk
    }

    pub fn build(mut self, rng: &mut (impl RngCore + CryptoRng)) -> Transaction {
        // 0. add dummy output if necessary
        // 1. compute fee
        // 2. make bullet-proof
        // 3. sign

        // TODO: move to a function
        let (bulletproof, output_commitments) = monero::make_bulletproof(
            rng,
            self.amounts.as_slice(),
            self.blinding_factors.as_slice(),
        )
        .unwrap();

        // TODO: move to ctor
        let (key_offsets, ring, commitment_ring) = self
            .decoy_inputs
            .iter()
            .copied()
            .map(
                |DecoyInput {
                     global_output_index,
                     key,
                     commitment,
                 }| { (VarInt(global_output_index), key, commitment) },
            )
            .chain(std::iter::once((
                VarInt(self.global_output_index),
                self.signing_pk,
                self.input_commitment,
            )))
            .sorted_by(|(a, ..), (b, ..)| Ord::cmp(a, b))
            .fold(
                (Vec::new(), Vec::new(), Vec::new()),
                |(mut key_offsets, mut ring, mut commitment_ring),
                 (key_offset, key, commitment)| {
                    key_offsets.push(key_offset);
                    ring.push(key);
                    commitment_ring.push(commitment);

                    (key_offsets, ring, commitment_ring)
                },
            );

        let ring: [EdwardsPoint; 11] = ring.try_into().unwrap();
        let commitment_ring = commitment_ring.try_into().unwrap();

        let (signing_index, _) = ring
            .iter()
            .find_position(|key| **key == self.signing_pk)
            .unwrap();

        let relative_key_offsets = to_relative_offsets(&key_offsets);
        let I = self.compute_key_image();
        let pseudo_out = self.compute_pseudo_out(output_commitments.as_slice());
        let fee = self.compute_fee();

        let prefix = TransactionPrefix {
            version: VarInt(2),
            unlock_time: Default::default(),
            inputs: vec![TxIn::ToKey {
                amount: VarInt(0),
                key_offsets: relative_key_offsets,
                k_image: KeyImage {
                    image: monero::cryptonote::hash::Hash(I.compress().to_bytes()),
                },
            }],
            outputs: self.outputs,
            extra: self.extra,
        };
        let rct_sig_base = RctSigBase {
            rct_type: RctType::Clsag,
            txn_fee: VarInt(fee),
            out_pk: output_commitments
                .iter()
                .map(|p| CtKey {
                    mask: Key {
                        key: p.compress().0,
                    },
                })
                .collect(),
            ecdh_info: self.ecdh_info,
            pseudo_outs: vec![], // legacy
        };
        let rct_sig_prunable = RctSigPrunable {
            range_sigs: vec![], // legacy
            bulletproofs: vec![bulletproof],
            MGs: vec![], // legacy
            Clsags: vec![],
            pseudo_outs: vec![Key {
                key: pseudo_out.compress().to_bytes(),
            }],
        };
        let mut transaction = Transaction {
            prefix,
            rct_signatures: RctSig {
                sig: Some(rct_sig_base),
                p: Some(rct_sig_prunable),
            },
            signatures: vec![], // legacy
        };

        let alpha = Scalar::random(rng);
        let fake_responses = random_array(|| Scalar::random(rng));
        let message = transaction.signature_hash().unwrap();

        let sig = monero::clsag::sign(
            message.as_fixed_bytes(),
            self.actual_signing_key,
            signing_index,
            self.H_p_pk,
            alpha,
            &ring,
            &commitment_ring,
            fake_responses,
            self.real_commitment_blinder - (self.blinding_factors.iter().sum::<Scalar>()),
            pseudo_out,
            alpha * ED25519_BASEPOINT_POINT,
            alpha * self.H_p_pk,
            I,
        );

        transaction.rct_signatures.p.as_mut().unwrap().Clsags = vec![sig];

        dbg!(transaction)
    }
}

#[derive(Debug, Copy, Clone)]
pub struct DecoyInput {
    global_output_index: u64,
    key: EdwardsPoint,
    commitment: EdwardsPoint,
}

fn to_relative_offsets(offsets: &[VarInt]) -> Vec<VarInt> {
    let vals = offsets.iter();
    let next_vals = offsets.iter().skip(1);

    let diffs = vals
        .zip(next_vals)
        .map(|(cur, next)| VarInt(next.0 - cur.0));
    iter::once(offsets[0].clone()).chain(diffs).collect()
}

fn random_array<T: Default + Copy, const N: usize>(rng: impl FnMut() -> T) -> [T; N] {
    let mut ring = [T::default(); N];
    ring[..].fill_with(rng);

    ring
}

#[async_trait::async_trait]
pub trait CalculateKeyOffsetBoundaries {
    async fn calculate_key_offset_boundaries(&self) -> Result<(VarInt, VarInt)>;
}

#[async_trait::async_trait]
pub trait FetchDecoyInputs {
    async fn fetch_decoy_inputs(&self, indices: [u64; 10]) -> Result<[DecoyInput; 10]>;
}

#[async_trait::async_trait]
impl CalculateKeyOffsetBoundaries for monerod::Client {
    /// Chooses 10 random key offsets for use within a new confidential
    /// transactions.
    ///
    /// Choosing these offsets randomly is not ideal for privacy, instead they
    /// should be chosen in a way that mimics a real spending pattern as much as
    /// possible.
    async fn calculate_key_offset_boundaries(&self) -> Result<(VarInt, VarInt)> {
        let latest_block = self.get_block_count().await?;
        let latest_spendable_block = latest_block.count - 100;

        let block: GetBlockResponse = self.get_block(latest_spendable_block).await?;

        let tx_hash = block
            .blob
            .tx_hashes
            .first()
            .copied()
            .unwrap_or_else(|| block.blob.miner_tx.hash());

        let indices = self.get_o_indexes(tx_hash).await?;

        let last_index = indices
            .o_indexes
            .into_iter()
            .max()
            .context("Expected at least one output index")?;
        // let oldest_index = last_index - (last_index / 100) * 40; // oldest index must be within last 40% TODO: CONFIRM THIS

        Ok((VarInt(0), VarInt(last_index)))
    }
}

#[async_trait::async_trait]
impl FetchDecoyInputs for monerod::Client {
    async fn fetch_decoy_inputs(&self, indices: [u64; 10]) -> Result<[DecoyInput; 10]> {
        let response = self
            .get_outs(
                indices
                    .iter()
                    .map(|offset| GetOutputsOut {
                        amount: 0,
                        index: *offset,
                    })
                    .collect(),
            )
            .await?;

        let inputs = response
            .outs
            .into_iter()
            .zip(indices.iter())
            .map(|(out_key, index)| {
                DecoyInput {
                    global_output_index: *index,
                    key: out_key.key.point.decompress().unwrap(), // TODO: should decompress on deserialization
                    commitment: CompressedEdwardsY(out_key.mask.key).decompress().unwrap(),
                }
            })
            .collect::<Vec<_>>()
            .try_into()
            .expect("exactly 10 elements guaranteed through type-safety of array");

        Ok(inputs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monero_harness::image::Monerod;
    use monero_rpc::monerod::Client;
    use testcontainers::clients::Cli;
    use testcontainers::Docker;

    #[test]
    fn calculate_relative_key_offsets() {
        let key_offsets = [
            VarInt(78),
            VarInt(81),
            VarInt(91),
            VarInt(91),
            VarInt(96),
            VarInt(98),
            VarInt(101),
            VarInt(112),
            VarInt(113),
            VarInt(114),
            VarInt(117),
        ];

        let relative_offsets = to_relative_offsets(&key_offsets);

        assert_eq!(
            &relative_offsets,
            &[
                VarInt(78),
                VarInt(3),
                VarInt(10),
                VarInt(0),
                VarInt(5),
                VarInt(2),
                VarInt(3),
                VarInt(11),
                VarInt(1),
                VarInt(1),
                VarInt(3),
            ]
        )
    }

    #[tokio::test]
    async fn get_outs_for_key_offsets() {
        let cli = Cli::default();
        let container = cli.run(Monerod::default());
        let rpc_client = Client::localhost(container.get_host_port(18081).unwrap()).unwrap();
        rpc_client.generateblocks(150, "498AVruCDWgP9Az9LjMm89VWjrBrSZ2W2K3HFBiyzzrRjUJWUcCVxvY1iitfuKoek2FdX6MKGAD9Qb1G1P8QgR5jPmmt3Vj".to_owned()).await.unwrap();
        // let wallet = Wallet {
        //     client: rpc_client.clone(),
        //     key: todo!(),
        // };
        //
        // let (lower, upper) = wallet.CalculateKeyOffsetBoundaries().await.unwrap();

        todo!("fix");
        // let result = rpc_client
        //     .get_outs(
        //         key_offsets
        //             .to_vec()
        //             .into_iter()
        //             .map(|varint| GetOutputsOut {
        //                 amount: 0,
        //                 index: varint.0,
        //             })
        //             .collect(),
        //     )
        //     .await
        //     .unwrap();

        // assert_eq!(result.outs.len(), 10);
    }
}
