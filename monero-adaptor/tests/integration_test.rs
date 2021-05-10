#![allow(non_snake_case)]

use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
use curve25519_dalek::edwards::{CompressedEdwardsY, EdwardsPoint};
use curve25519_dalek::scalar::Scalar;
use hash_edwards_to_edwards::hash_point_to_point;
use monero::blockdata::transaction::{ExtraField, KeyImage, SubField, TxOutTarget};
use monero::cryptonote::hash::Hashable;
use monero::cryptonote::onetime_key::{KeyGenerator, MONERO_MUL_FACTOR};
use monero::util::key::H;
use monero::util::ringct::{EcdhInfo, RctSig, RctSigBase, RctSigPrunable, RctType};
use monero::{
    PrivateKey, PublicKey, Transaction, TransactionPrefix, TxIn, TxOut, VarInt, ViewPair,
};
use monero_harness::Monero;
use monero_rpc::monerod::{GetOutputsOut, MonerodRpc};
use monero_wallet::MonerodClientExt;
use rand::rngs::OsRng;
use rand::{thread_rng, CryptoRng, Rng, SeedableRng};
use std::convert::TryInto;
use std::iter;
use testcontainers::clients::Cli;

#[tokio::test]
async fn monerod_integration_test() {
    let mut rng = rand::rngs::StdRng::from_seed([0u8; 32]);

    let cli = Cli::default();
    let (monero, _monerod_container, _monero_wallet_rpc_containers) =
        Monero::new(&cli, vec![]).await.unwrap();

    let s_a = curve25519_dalek::scalar::Scalar::random(&mut rng);
    let s_b = curve25519_dalek::scalar::Scalar::random(&mut rng);
    let lock_kp = monero::KeyPair {
        view: monero::PrivateKey::from_scalar(curve25519_dalek::scalar::Scalar::random(&mut rng)),
        spend: monero::PrivateKey::from_scalar(s_a + s_b),
    };

    let lock_amount = 1_000_000_000_000;
    let fee = 400_000_000;
    let spend_amount = lock_amount - fee;

    let lock_address = monero::Address::from_keypair(monero::Network::Mainnet, &lock_kp);

    dbg!(lock_address.to_string()); // 45BcRKAHaA4b5A9SdamF2f1w7zk1mKkBPhaqVoDWzuAtMoSAytzm5A6b2fE6ruupkAFmStrQzdojUExt96mR3oiiSKp8Exf

    monero.init_miner().await.unwrap();
    let wallet = monero.wallet("miner").expect("wallet to exist");

    let transfer = wallet
        .transfer(&lock_address.to_string(), lock_amount)
        .await
        .expect("lock to succeed");

    let client = monero.monerod().client();

    let miner_address = wallet
        .address()
        .await
        .expect("miner address to exist")
        .address;
    client
        .generateblocks(10, miner_address)
        .await
        .expect("can generate blocks");

    let lock_tx = transfer.tx_hash.parse().unwrap();

    let o_indexes_response = client.get_o_indexes(lock_tx).await.unwrap();

    let transaction = client
        .get_transactions(&[lock_tx])
        .await
        .unwrap()
        .pop()
        .unwrap();

    dbg!(&transaction.prefix.inputs);

    let viewpair = ViewPair::from(&lock_kp);

    let our_output = transaction
        .check_outputs(&viewpair, 0..1, 0..1)
        .expect("to have outputs in this transaction")
        .pop()
        .expect("to own at least one output");
    let actual_lock_amount = transaction.get_amount(&viewpair, &our_output).unwrap();

    assert_eq!(actual_lock_amount, lock_amount);

    let real_key_offset = o_indexes_response.o_indexes[our_output.index];

    let (lower, upper) = client.calculate_key_offset_boundaries().await.unwrap();

    let mut key_offsets = Vec::with_capacity(11);
    key_offsets.push(VarInt(real_key_offset));

    for _ in 0..10 {
        loop {
            let decoy_offset = VarInt(rng.gen_range(lower.0, upper.0));

            if key_offsets.contains(&decoy_offset) {
                continue;
            }

            key_offsets.push(decoy_offset);
            break;
        }
    }

    dbg!(&key_offsets);

    let response = client
        .get_outs(
            key_offsets
                .iter()
                .map(|offset| GetOutputsOut {
                    amount: 0,
                    index: offset.0,
                })
                .collect(),
        )
        .await
        .unwrap();

    dbg!(&response);

    let ring = response
        .outs
        .iter()
        .map(|out| out.key.point.decompress().unwrap())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();

    key_offsets.sort();

    let relative_key_offsets = to_relative_offsets(&key_offsets);

    dbg!(&relative_key_offsets);

    let target_address = "498AVruCDWgP9Az9LjMm89VWjrBrSZ2W2K3HFBiyzzrRjUJWUcCVxvY1iitfuKoek2FdX6MKGAD9Qb1G1P8QgR5jPmmt3Vj".parse::<monero::Address>().unwrap();

    let ecdh_key_0 = PrivateKey::random(&mut rng);
    let (ecdh_info_0, out_blinding_0) = EcdhInfo::new_bulletproof(spend_amount, ecdh_key_0.scalar);

    let ecdh_key_1 = PrivateKey::random(&mut rng);
    let (ecdh_info_1, out_blinding_1) = EcdhInfo::new_bulletproof(spend_amount, ecdh_key_1.scalar);

    let (bulletproof, out_pk) = monero::make_bulletproof(&mut rng, &[spend_amount, 0], &[
        out_blinding_0,
        out_blinding_1,
    ])
    .unwrap();

    let k_image = {
        let k = lock_kp.spend.scalar;
        let K = ViewPair::from(&lock_kp).spend.point;

        let k_image = k * hash_point_to_point(K.decompress().unwrap());
        KeyImage {
            image: monero::cryptonote::hash::Hash(k_image.compress().to_bytes()),
        }
    };

    let prefix = TransactionPrefix {
        version: VarInt(2),
        unlock_time: Default::default(),
        inputs: vec![TxIn::ToKey {
            amount: VarInt(0),
            key_offsets: relative_key_offsets,
            k_image,
        }],
        outputs: vec![
            TxOut {
                amount: VarInt(0),
                target: TxOutTarget::ToKey {
                    key: KeyGenerator::from_random(
                        target_address.public_view,
                        target_address.public_spend,
                        ecdh_key_0,
                    )
                    .one_time_key(0), // TODO: This must be the output index
                },
            },
            TxOut {
                amount: VarInt(0),
                target: TxOutTarget::ToKey {
                    key: KeyGenerator::from_random(
                        target_address.public_view,
                        target_address.public_spend,
                        ecdh_key_1,
                    )
                    .one_time_key(1), // TODO: This must be the output index
                },
            },
        ],
        extra: ExtraField(vec![
            SubField::TxPublicKey(PublicKey::from_private_key(&ecdh_key_0)),
            SubField::TxPublicKey(PublicKey::from_private_key(&ecdh_key_1)),
        ]),
    };

    // assert_eq!(prefix.hash(),
    // "c3ded4d1a8cddd4f76c09b63edff4e312e759b3afc46beda4e1fd75c9c68d997".parse().
    // unwrap());

    let s_prime_a = s_a
        + KeyGenerator::from_key(&viewpair, our_output.tx_pubkey)
            .get_rvn_scalar(our_output.index)
            .scalar;

    let commitment_ring = response
        .outs
        .iter()
        .map(|out| CompressedEdwardsY(out.mask.key).decompress().unwrap())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();

    let out_pk = out_pk
        .into_iter()
        .map(|p| (p.decompress().unwrap() * Scalar::from(MONERO_MUL_FACTOR)).compress())
        .collect::<Vec<_>>();

    let fee_key = Scalar::from(fee) * H.point.decompress().unwrap();

    let pseudo_out = fee_key + out_pk[0].decompress().unwrap() + out_pk[1].decompress().unwrap();

    let (_, real_commitment_blinder) = transaction.clone().rct_signatures.sig.unwrap().ecdh_info
        [our_output.index]
        .open_commitment(&viewpair, &our_output.tx_pubkey, our_output.index);

    let (adaptor_sig, adaptor) = single_party_adaptor_sig(
        s_prime_a,
        s_b,
        ring,
        commitment_ring,
        pseudo_out,
        real_commitment_blinder,
        out_blinding_0 + out_blinding_1, /* TODO: These haven't been multiplied by 8. Is that
                                          * correct? */
        &prefix.hash().to_bytes(),
        &mut rng,
    );

    let sig = adaptor_sig.adapt(adaptor);

    let out_pk = out_pk
        .iter()
        .map(|c| monero::util::ringct::CtKey {
            mask: monero::util::ringct::Key { key: c.to_bytes() },
        })
        .collect::<Vec<_>>();

    let transaction = Transaction {
        prefix,
        signatures: Vec::new(),
        rct_signatures: RctSig {
            sig: Some(RctSigBase {
                rct_type: RctType::Clsag,
                txn_fee: VarInt(fee),
                pseudo_outs: Vec::new(),
                ecdh_info: vec![ecdh_info_0, ecdh_info_1],
                out_pk,
            }),
            p: Some(RctSigPrunable {
                range_sigs: Vec::new(),
                bulletproofs: vec![bulletproof],
                MGs: Vec::new(),
                Clsags: vec![sig.into()],
                pseudo_outs: vec![monero::util::ringct::Key {
                    key: pseudo_out.compress().0,
                }],
            }),
        },
    };

    client.send_raw_transaction(transaction).await.unwrap();
}

fn to_relative_offsets(offsets: &[VarInt]) -> Vec<VarInt> {
    let vals = offsets.iter();
    let next_vals = offsets.iter().skip(1);

    let diffs = vals
        .zip(next_vals)
        .map(|(cur, next)| VarInt(next.0 - cur.0));
    iter::once(offsets[0].clone()).chain(diffs).collect()
}

/// First element of ring is the real pk.
fn single_party_adaptor_sig(
    s_prime_a: Scalar,
    s_b: Scalar,
    ring: [EdwardsPoint; monero_adaptor::RING_SIZE],
    commitment_ring: [EdwardsPoint; monero_adaptor::RING_SIZE],
    pseudo_output_commitment: EdwardsPoint,
    real_commitment_blinding: Scalar,
    pseudo_output_commitment_blinding: Scalar,
    msg: &[u8; 32],
    rng: &mut (impl Rng + CryptoRng),
) -> (monero_adaptor::AdaptorSignature, Scalar) {
    let (r_a, R_a, R_prime_a) = {
        let r_a = Scalar::random(&mut OsRng);
        let R_a = r_a * ED25519_BASEPOINT_POINT;

        let pk_hashed_to_point = hash_point_to_point(ring[0]);

        let R_prime_a = r_a * pk_hashed_to_point;

        (r_a, R_a, R_prime_a)
    };

    let alice = monero_adaptor::Alice0::new(
        ring,
        *msg,
        commitment_ring,
        pseudo_output_commitment,
        R_a,
        R_prime_a,
        s_prime_a,
        rng,
    )
    .unwrap();
    let bob = monero_adaptor::Bob0::new(
        ring,
        *msg,
        commitment_ring,
        pseudo_output_commitment,
        R_a,
        R_prime_a,
        s_b,
        rng,
    )
    .unwrap();

    let msg = alice.next_message(rng);
    let bob = bob.receive(msg);

    let z = real_commitment_blinding - pseudo_output_commitment_blinding;

    let msg = bob.next_message(rng);
    let alice = alice.receive(msg, z).unwrap();

    let msg = alice.next_message();
    let bob = bob.receive(msg, z).unwrap();

    let msg = bob.next_message();
    let alice = alice.receive(msg);

    (alice.adaptor_sig, r_a)
}

#[cfg(test)]
mod tests {
    use super::*;

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

        assert_eq!(&relative_offsets, &[
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
        ])
    }
}
