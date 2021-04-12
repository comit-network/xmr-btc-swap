#[cfg(test)]
mod tests {
    use curve25519_dalek::scalar::Scalar;
    use monero::blockdata::transaction::TxOutTarget;
    use monero::blockdata::TransactionPrefix;
    use monero::consensus::encode::VarInt;
    use monero::{TxIn, TxOut};
    use monero_harness::Monero;
    use testcontainers::*;

    #[tokio::test]
    async fn can_broadcast_locally_signed_transaction() {
        let cli = clients::Cli::default();
        let (monero, containers) = Monero::new(&cli, vec!["Alice"]).await.unwrap();

        let view_key = monero::PrivateKey::from_scalar(Scalar::random(&mut rand::thread_rng()));
        let spend_key = monero::PrivateKey::from_scalar(Scalar::random(&mut rand::thread_rng()));

        let public_view_key = monero::PublicKey::from_private_key(&view_key);
        let public_spend_key = monero::PublicKey::from_private_key(&spend_key);

        let address =
            monero::Address::standard(monero::Network::Stagenet, public_spend_key, public_view_key);
        let transfer = monero
            .wallet("miner")
            .unwrap()
            .client()
            .transfer(0, 100_000, &address.to_string())
            .await
            .unwrap();

        // [k_image, k_image + offset_0, k_image + offset_0 + offset_1, ..]
        let mut transaction = monero::Transaction::default();
        transaction.prefix.version = VarInt(2);
        transaction.prefix.inputs.push(TxIn::ToKey {
            amount: VarInt(0),
            k_image: todo!(),
            key_offsets: Vec::new(),
        });
        transaction.prefix.outputs.push(TxOut {
            amount: VarInt(0),
            target: TxOutTarget::ToKey {
                key: monero
                    .wallet("alice")
                    .unwrap()
                    .client()
                    .get_address(0)
                    .await
                    .unwrap()
                    .address
                    .parse::<monero::Address>()
                    .unwrap()
                    .public_spend,
            },
        });
    }
}
