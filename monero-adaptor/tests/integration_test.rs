use monero_rpc::monerod;
use rand::{SeedableRng, Rng};
use monero_rpc::monerod::{MonerodRpc, GetOutputsOut};
use monero::{Transaction, TransactionPrefix, VarInt, TxIn};
use std::iter;
use itertools::Itertools;

// [0u8; 32] = 466iKkx7MqVGD46dje3kwvSQRMfhNCvGaXTRATbQgz7kS8XTMmRmoTw9oJRRj523kTdQj8gXnF2xU9fmEPy9WXTr6pwetQj
// [1u8; 32] = 47HCnKkBEeYfX5pScvBETAKdjBEPN7FcXEJPUqDPzWGCc6wC8VAdS8CjdtgKuSaY72K8fkoswjp176vbSPS8hzS17EZv8gj

#[tokio::test]
async fn monerod_integration_test() {
    let client = monerod::Client::localhost(18081).unwrap();
    let mut rng = rand::rngs::StdRng::from_seed([0u8; 32]);

    let wallet = monero_wallet::Wallet::new_random(client.clone(), &mut rng);

    let spend_tx = "d5d82405a655ebf721e32c8ef2f8b77f2476dd7cbdb06c05b9735abdf9a2a927".parse().unwrap();

    let mut o_indexes_response = client.get_o_indexes(spend_tx).await.unwrap();

    let real_key_offset = o_indexes_response.o_indexes.pop().unwrap();

    // let (lower, upper) = dbg!(wallet.calculate_key_offset_boundaries().await.unwrap());
    let (lower, upper) = (
        VarInt(77),
        VarInt(117),
    );

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
    key_offsets.sort();

    let outs = client.get_outs(key_offsets.iter().map(|offset| GetOutputsOut {
        amount: 0,
        index: offset.0
    }).collect()).await.unwrap();

    dbg!(outs);

    // [
    //         OutKey {
    //             height: 80,
    //             key: 90160faa3f57077ee05eaae797d95eae96d6d6b31f02df274ded85a63b5f2987,
    //             mask: be8aa4ab10aab1cd1920b83243ebdfb84c2275dc0eaeee8b7e202c3d1f314c92,
    //             txid: 0x1062a5b667023ebbb39fbc8898ea43488f83fbf877c790895c3882d4f1ef65cd,
    //             unlocked: false,
    //         },
    //         OutKey {
    //             height: 85,
    //             key: 97d00aca5023c8092b1070442169b1088f533904e2938ad52696324ee1fbb780,
    //             mask: 1b68a61d530c63749269ee5b4b4ff251944489aa636e92b78dac78a5b0f964c7,
    //             txid: 0x505d33dd8e5e891e5e10010a8188d3f9303d1939b3801a3385e6acd15b81ab57,
    //             unlocked: false,
    //         },
    //         OutKey {
    //             height: 89,
    //             key: 3645d682a5a13e4f58fb3d711f247237251ba8f38d2bf6f00dc974db14e1ceac,
    //             mask: 2e3e47603f364de65dfce86090f9943a0f702f53cdd2fc03ab537c899a5eae67,
    //             txid: 0x96e730e7aebfbf9863f18541c12ab49fb6b9dd44eff85369b0b70f85a4afc37b,
    //             unlocked: false,
    //         },
    //         OutKey {
    //             height: 90,
    //             key: 5e0f69d43987c49d5ded18aa20ea81df9aa133a30e917ec7549e2f29978674ec,
    //             mask: d9f4bf0287eefb653575b1c7ed3a377f45c29318fbb5f91989ffa9c986acbeff,
    //             txid: 0x4855aa4b17e847b928fb452a762302962b80905ea83708f83a87d18884dc3adc,
    //             unlocked: false,
    //         },
    //         OutKey {
    //             height: 91,
    //             key: f3d362c830e20a365e6f2f13e05d21dabcb6ac9517351dcaa66da64d38af8a28,
    //             mask: c8e768cbf939f780f6197b8c0d7a1baa3caf0407fdee3f555fd791a740e85e6a,
    //             txid: 0x495d961a96db30d0bcbc89acda693ceb371ecf56b4b036f8c0511b5bf5688579,
    //             unlocked: false,
    //         },
    //         OutKey {
    //             height: 96,
    //             key: 1f51a5f61d7d8e2ae431488c62e4e6523c02856a03af7d0c51929fa9c94da0b8,
    //             mask: 167200b570a7b31bf23b3f8c111b03d073f128895a33a9de8ed2b8b839b42ff9,
    //             txid: 0xdb7ee874506e2f75d52e2ae7f4d140dbd37cceba870aef2ffe25a417094991d4,
    //             unlocked: false,
    //         },
    //         OutKey {
    //             height: 105,
    //             key: c34378e67680bd42964457294a7587ab8c4fa813c172f16faebd6f151db39369,
    //             mask: f034b8ff2cbd2729a7c19c0cc59a053fcbe1123f10acb0ec9e86bf122f0d3b12,
    //             txid: 0x5e2ca0b269c56e370cb8b243f4a7c5656195471dcb643a99036050fbb212f248,
    //             unlocked: false,
    //         },
    //         OutKey {
    //             height: 108,
    //             key: f8cf697bb285cbd61586543b2cd9e70f6b030348586d6e24c2a82cd8a4b4c8db,
    //             mask: 9ad72a2f1867a99367f045a512b7474234e0e08e39ba4f8fbd961d076348ee28,
    //             txid: 0x9a27def957d31482922cfd98a8422e92eba2a40f5ea1c285b5ac708b359f1a8d,
    //             unlocked: false,
    //         },
    //         OutKey {
    //             height: 109,
    //             key: 5b90003c16b591249d968cf401cd5ea99392963b401476746bad28606362bd42,
    //             mask: 69a62e221e990bc91453dfaee01dfc307dff680a6b270944598014df32097563,
    //             txid: 0xfc6b1a98cadbf19e84a0dccff53c34dbd4cf1e27c3558c913f7b3512d57d42b8,
    //             unlocked: false,
    //         },
    //         OutKey {
    //             height: 113,
    //             key: ad46eef99661c1e93ba517d3e541eebf9c42ba015566ff5dcc3a689edaf8f2ac,
    //             mask: b3012a4a5c0b3b465246e9671b7cdb84f817bedcc1ab9015aeff1f6ccffebb0c,
    //             txid: 0xf8a557dd14b6714eab5661b35944c4b09d0f8a9ef82b31ffb19244f7dfd127da,
    //             unlocked: false,
    //         },
    //         OutKey {
    //             height: 116,
    //             key: 76f445a4ff9a957d979f910040938a25b01759c7aad3605193502b35cdc8ff58,
    //             mask: a2091a955e819696b4526ae54f7dcf69f9fe7a42a0edcdb69f787437fc73244d,
    //             txid: 0xd5d82405a655ebf721e32c8ef2f8b77f2476dd7cbdb06c05b9735abdf9a2a927,
    //             unlocked: true,
    //         },
    //     ],

    let relative_key_offsets = to_relative_offsets(&key_offsets);

    let transaction = Transaction {
        prefix: TransactionPrefix {
            version: VarInt(2),
            unlock_time: Default::default(),
            inputs: vec![
                TxIn::ToKey {
                    amount: VarInt(0),
                    key_offsets: relative_key_offsets,
                    k_image: todo!()
                }
            ],
            outputs: vec![], // 498AVruCDWgP9Az9LjMm89VWjrBrSZ2W2K3HFBiyzzrRjUJWUcCVxvY1iitfuKoek2FdX6MKGAD9Qb1G1P8QgR5jPmmt3Vj
            extra: Default::default()
        },
        signatures: vec![],
        rct_signatures: Default::default()
    };
}

fn to_relative_offsets(offsets: &[VarInt]) -> Vec<VarInt> {
    let vals = offsets.iter();
    let next_vals = offsets.iter().skip(1);

    let diffs = vals.zip(next_vals).map(|(cur, next)| VarInt(next.0 - cur.0));
    iter::once(offsets[0].clone()).chain(diffs).collect()
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
