#![allow(non_snake_case)]

use monero::ViewPair;
use monero_harness::Monero;
use monero_rpc::monerod::MonerodRpc;
use monero_wallet::{ConfidentialTransactionBuilder, MonerodClientExt};
use rand::{Rng, SeedableRng};
use std::convert::TryInto;
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

    let spend_amount = 999600000000;

    let lock_address = monero::Address::from_keypair(monero::Network::Mainnet, &lock_kp);

    monero.init_miner().await.unwrap();
    let wallet = monero.wallet("miner").expect("wallet to exist");

    let transfer = wallet
        .transfer(&lock_address.to_string(), 1_000_000_000_000)
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

    let lock_tx_hash = transfer.tx_hash.parse().unwrap();

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

    let target_address = "498AVruCDWgP9Az9LjMm89VWjrBrSZ2W2K3HFBiyzzrRjUJWUcCVxvY1iitfuKoek2FdX6MKGAD9Qb1G1P8QgR5jPmmt3Vj".parse().unwrap();

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
