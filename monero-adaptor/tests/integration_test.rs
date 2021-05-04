#![allow(non_snake_case)]

use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
use curve25519_dalek::edwards::EdwardsPoint;
use curve25519_dalek::scalar::Scalar;
use hash_edwards_to_edwards::hash_point_to_point;
use itertools::Itertools;
use monero::blockdata::transaction::{ExtraField, SubField, TxOutTarget};
use monero::cryptonote::hash::Hashable;
use monero::cryptonote::onetime_key::KeyGenerator;
use monero::util::ringct::{EcdhInfo, RctSig, RctSigBase, RctSigPrunable, RctType};
use monero::{PrivateKey, PublicKey};
use monero::{Transaction, TransactionPrefix, TxIn, TxOut, VarInt};
use monero_rpc::monerod;
use monero_rpc::monerod::{GetOutputsOut, MonerodRpc};
use monero_wallet::{MonerodClientExt, Wallet};
use rand::rngs::OsRng;
use rand::{Rng, SeedableRng};
use std::convert::TryInto;
use std::iter;

// [0u8; 32] = 466iKkx7MqVGD46dje3kwvSQRMfhNCvGaXTRATbQgz7kS8XTMmRmoTw9oJRRj523kTdQj8gXnF2xU9fmEPy9WXTr6pwetQj
// [1u8; 32] = 47HCnKkBEeYfX5pScvBETAKdjBEPN7FcXEJPUqDPzWGCc6wC8VAdS8CjdtgKuSaY72K8fkoswjp176vbSPS8hzS17EZv8gj

#[tokio::test]
async fn make_blocks() {
    let client = monerod::Client::localhost(18081).unwrap();

    client.generateblocks(10, "47HCnKkBEeYfX5pScvBETAKdjBEPN7FcXEJPUqDPzWGCc6wC8VAdS8CjdtgKuSaY72K8fkoswjp176vbSPS8hzS17EZv8gj".to_owned()).await.unwrap();
}

#[tokio::test]
async fn monerod_integration_test() {
    let client = monerod::Client::localhost(18081).unwrap();
    let mut rng = rand::rngs::StdRng::from_seed([0u8; 32]);

    let s_prime_a = curve25519_dalek::scalar::Scalar::random(&mut rng);
    let s_b = curve25519_dalek::scalar::Scalar::random(&mut rng);
    let lock_kp = monero::KeyPair {
        view: monero::PrivateKey::from_scalar(curve25519_dalek::scalar::Scalar::random(&mut rng)),
        spend: monero::PrivateKey::from_scalar(s_prime_a + s_b),
    };

    let lock_address = monero::Address::from_keypair(monero::Network::Mainnet, &lock_kp);

    let spend_tx = "c9b8c57097fe3af0bffcc7470355afa804be2cad0c559a99506ac040cb93d62d"
        .parse()
        .unwrap();

    let mut o_indexes_response = client.get_o_indexes(spend_tx).await.unwrap();

    let real_key_offset = o_indexes_response.o_indexes.pop().unwrap();

    let (lower, upper) = client.calculate_key_offset_boundaries().await.unwrap();

    let mut key_offsets = Vec::with_capacity(11);
    key_offsets.push(VarInt(real_key_offset));

    for i in 0..10 {
        loop {
            let decoy_offset = VarInt(rng.gen_range(lower.0, upper.0));

            if key_offsets.contains(&decoy_offset) {
                continue;
            }

            key_offsets.push(decoy_offset);
            break;
        }
    }

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
    let ring = response
        .outs
        .iter()
        .map(|out| out.key.point.decompress().unwrap())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();

    key_offsets.sort();

    let relative_key_offsets = to_relative_offsets(&key_offsets);

    let amount = 10_000_000;
    let fee = 10_000;
    // TODO: Pay lock amount (= amount + fee) to shared address (s_prime_a + s_b)

    let (bulletproof, out_pk, out_blindings) =
        monero::make_bulletproof(&mut rng, &[amount]).unwrap();
    let out_pk = out_pk
        .iter()
        .map(|c| monero::util::ringct::CtKey {
            mask: monero::util::ringct::Key { key: c.to_bytes() },
        })
        .collect();

    let target_address = "498AVruCDWgP9Az9LjMm89VWjrBrSZ2W2K3HFBiyzzrRjUJWUcCVxvY1iitfuKoek2FdX6MKGAD9Qb1G1P8QgR5jPmmt3Vj".parse::<monero::Address>().unwrap();

    let ecdh_key = PrivateKey::random(&mut rng);

    let prefix = TransactionPrefix {
        version: VarInt(2),
        unlock_time: Default::default(),
        inputs: vec![TxIn::ToKey {
            amount: VarInt(0),
            key_offsets: relative_key_offsets,
            k_image: todo!(),
        }],
        outputs: vec![TxOut {
            amount: VarInt(0),
            target: TxOutTarget::ToKey {
                key: KeyGenerator::from_random(
                    target_address.public_view,
                    target_address.public_spend,
                    ecdh_key,
                )
                .one_time_key(0),
            },
        }],
        extra: ExtraField(vec![SubField::TxPublicKey(PublicKey::from_private_key(
            &ecdh_key,
        ))]),
    };

    let (adaptor_sig, adaptor) =
        single_party_adaptor_sig(s_prime_a, s_b, ring, &prefix.hash().to_bytes());

    let sig = adaptor_sig.adapt(adaptor);

    let transaction = Transaction {
        prefix,
        signatures: Vec::new(),
        rct_signatures: RctSig {
            sig: Some(RctSigBase {
                rct_type: RctType::Clsag,
                txn_fee: VarInt(fee),
                pseudo_outs: Vec::new(),
                ecdh_info: todo!(),
                out_pk,
            }),
            p: Some(RctSigPrunable {
                range_sigs: Vec::new(),
                bulletproofs: vec![bulletproof],
                MGs: Vec::new(),
                Clsags: vec![sig.into()],
                pseudo_outs: todo!("out_blindings + pseudo_outs == 0, 1 pseudo out per input: calculated by input amount * G + H * 'random blinding factor'"),
            }),
        },
    };

    todo!("broadcast transaction")
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
    msg: &[u8; 32],
) -> (monero_adaptor::AdaptorSignature, Scalar) {
    let (r_a, R_a, R_prime_a) = {
        let r_a = Scalar::random(&mut OsRng);
        let R_a = r_a * ED25519_BASEPOINT_POINT;

        let pk_hashed_to_point = hash_point_to_point(ring[0]);

        let R_prime_a = r_a * pk_hashed_to_point;

        (r_a, R_a, R_prime_a)
    };

    let alice = monero_adaptor::Alice0::new(ring, *msg, R_a, R_prime_a, s_prime_a).unwrap();
    let bob = monero_adaptor::Bob0::new(ring, *msg, R_a, R_prime_a, s_b).unwrap();

    let msg = alice.next_message();
    let bob = bob.receive(msg);

    let msg = bob.next_message();
    let alice = alice.receive(msg).unwrap();

    let msg = alice.next_message();
    let bob = bob.receive(msg).unwrap();

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
}
