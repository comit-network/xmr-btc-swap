#![allow(non_snake_case)]

use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
use curve25519_dalek::edwards::CompressedEdwardsY;
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
use rand::{Rng, SeedableRng};
use std::convert::TryInto;
use std::iter;
use testcontainers::clients::Cli;

#[tokio::test]
async fn monerod_integration_test() {
    let mut rng = rand::rngs::StdRng::from_seed([0u8; 32]);

    let cli = Cli::default();
    let (monero, _monerod_container, _monero_wallet_rpc_containers) =
        Monero::new(&cli, vec![]).await.unwrap();

    let signing_key = curve25519_dalek::scalar::Scalar::random(&mut rng);
    let lock_kp = monero::KeyPair {
        view: monero::PrivateKey::from_scalar(curve25519_dalek::scalar::Scalar::random(&mut rng)),
        spend: monero::PrivateKey::from_scalar(signing_key),
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

    let (bulletproof, out_pk) = monero::make_bulletproof(
        &mut rng,
        &[spend_amount, 0],
        &[out_blinding_0, out_blinding_1],
    )
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

    let signing_key = signing_key
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

    let H_p_pk = hash_point_to_point(signing_key * ED25519_BASEPOINT_POINT);
    let alpha = Scalar::random(&mut rng);

    let sig = monero_adaptor::clsag::sign(
        &prefix.hash().to_bytes(),
        signing_key,
        H_p_pk,
        alpha,
        &ring,
        &commitment_ring,
        random_array(|| Scalar::random(&mut rng)),
        real_commitment_blinder - (out_blinding_0 + out_blinding_1), // * Scalar::from(MONERO_MUL_FACTOR), TODO DOESN'T VERIFY WITH THIS
        pseudo_out,
        alpha * ED25519_BASEPOINT_POINT,
        alpha * H_p_pk,
        signing_key * H_p_pk,
    );
    assert!(monero_adaptor::clsag::verify(
        &sig,
        &prefix.hash().to_bytes(),
        &ring,
        &commitment_ring,
        pseudo_out
    ));

    sig.responses.iter().enumerate().for_each(|(i, res)| {
        println!(
            r#"epee::string_tools::hex_to_pod("{}", clsag.s[{}]);"#,
            hex::encode(res.as_bytes()),
            i
        );
    });
    println!(
        r#"epee::string_tools::hex_to_pod("{}", clsag.c1);"#,
        hex::encode(sig.h_0.as_bytes())
    );
    println!(
        r#"epee::string_tools::hex_to_pod("{}", clsag.D);"#,
        hex::encode(sig.D.compress().as_bytes())
    );
    println!(
        r#"epee::string_tools::hex_to_pod("{}", clsag.I);"#,
        hex::encode(sig.I.compress().to_bytes())
    );
    println!(
        r#"epee::string_tools::hex_to_pod("{}", msg);"#,
        hex::encode(&prefix.hash().to_bytes())
    );

    ring.iter()
        .zip(commitment_ring.iter())
        .enumerate()
        .for_each(|(i, (pk, c))| {
            println!(
                r#"epee::string_tools::hex_to_pod("{}", pubs[{}].dest);"#,
                hex::encode(&pk.compress().to_bytes()),
                i
            );
            println!(
                r#"epee::string_tools::hex_to_pod("{}", pubs[{}].mask);"#,
                hex::encode(&c.compress().to_bytes()),
                i
            );
        });

    println!(
        r#"epee::string_tools::hex_to_pod("{}", Cout);"#,
        hex::encode(pseudo_out.compress().to_bytes())
    );

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

fn random_array<T: Default + Copy, const N: usize>(rng: impl FnMut() -> T) -> [T; N] {
    let mut ring = [T::default(); N];
    ring[..].fill_with(rng);

    ring
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
