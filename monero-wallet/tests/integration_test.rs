#![allow(non_snake_case)]

use monero::ViewPair;
use monero_harness::Monero;
use monero_rpc::monerod::{Client, MonerodRpc};
use monero_wallet::{
    CalculateKeyOffsetBoundaries, ConfidentialTransactionBuilder, FetchDecoyInputs,
};
use rand::{Rng, SeedableRng};
use std::convert::TryInto;
use testcontainers::clients::Cli;

#[tokio::test]
async fn monerod_integration_test() {
    let mut rng = rand::rngs::StdRng::from_seed([0u8; 32]);

    let s_a = monero::PrivateKey::from_canonical_bytes([
        146, 74, 223, 240, 209, 247, 144, 163, 20, 194, 1, 57, 226, 43, 37, 91, 207, 19, 121, 71,
        156, 217, 25, 138, 86, 22, 4, 40, 160, 103, 146, 1,
    ])
    .unwrap();
    let s_b = monero::PrivateKey::from_canonical_bytes([
        172, 121, 31, 191, 236, 27, 215, 81, 213, 34, 185, 248, 161, 212, 138, 11, 73, 79, 251,
        205, 128, 70, 58, 232, 37, 71, 1, 110, 72, 114, 47, 6,
    ])
    .unwrap();

    let lock_kp = monero::KeyPair {
        view: monero::PrivateKey::from_canonical_bytes([
            167, 4, 78, 117, 31, 113, 199, 197, 193, 40, 228, 194, 1, 190, 82, 210, 4, 141, 166,
            109, 55, 64, 127, 65, 181, 248, 126, 146, 224, 241, 111, 13,
        ])
        .unwrap(),
        spend: s_a + s_b,
    };

    let client = Client::new("localhost".to_string(), 38081).unwrap();

    let lock_tx_hash = "09e361acb3e6e71d627a945a30672776a6f8fec7c97f4cae5e09b0780b75c158"
        .parse()
        .unwrap();

    let lock_tx = client
        .get_transactions(&[lock_tx_hash])
        .await
        .unwrap()
        .pop()
        .unwrap();

    let output_indices = client.get_o_indexes(lock_tx_hash).await.unwrap().o_indexes;

    let lock_vp = ViewPair::from(&lock_kp);

    let input_to_spend = lock_tx
        .check_outputs(&lock_vp, 0..1, 0..1)
        .unwrap()
        .pop()
        .unwrap();

    dbg!(input_to_spend.amount().unwrap());

    let global_output_index = output_indices[input_to_spend.index()];

    let (lower, upper) = client.calculate_key_offset_boundaries().await.unwrap();

    let mut decoy_indices = Vec::with_capacity(10);
    for _ in 0..10 {
        loop {
            let decoy_index = rng.gen_range(lower.0, upper.0);

            if decoy_indices.contains(&decoy_index) && decoy_index != global_output_index {
                continue;
            }

            decoy_indices.push(decoy_index);
            break;
        }
    }

    let decoy_inputs = client
        .fetch_decoy_inputs(decoy_indices.try_into().unwrap())
        .await
        .unwrap();

    let target_address = "58hKkN5JrirdNehmTXaHhTEg3N5zRYZ6Wb5g5jwDk3wRC4rtNCJvx7hENsbLmfPakC3spGhciosagdVbSqq9vfXsV3zusCn".parse().unwrap();

    let spend_amount = 149720581473;

    let transaction = ConfidentialTransactionBuilder::new(
        input_to_spend,
        global_output_index,
        decoy_inputs,
        lock_kp,
    )
    .with_output(target_address, spend_amount, &mut rng)
    .with_output(target_address, 0, &mut rng) // TODO: Do this inside `build`
    .build(&mut rng);

    client.send_raw_transaction(transaction).await.unwrap();
}
