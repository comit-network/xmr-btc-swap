use anyhow::{Context, Result};
use monero::consensus::encode::VarInt;
use monero::cryptonote::hash::Hashable;
use monero_rpc::monerod;
use monero_rpc::monerod::{GetBlockResponse, MonerodRpc as _};
use rand::{CryptoRng, RngCore};

pub struct Wallet {
    key: monero::KeyPair,
}

impl Wallet {
    pub fn new_random<R: CryptoRng + RngCore>(rng: &mut R) -> Self {
        Self {
            key: monero::KeyPair {
                view: monero::PrivateKey::from_scalar(curve25519_dalek::scalar::Scalar::random(
                    rng,
                )),
                spend: monero::PrivateKey::from_scalar(curve25519_dalek::scalar::Scalar::random(
                    rng,
                )),
            },
        }
    }

    pub fn get_address(&self, network: monero::Network) -> monero::Address {
        monero::Address::from_keypair(network, &self.key)
    }
}

#[async_trait::async_trait]
pub trait MonerodClientExt {
    async fn calculate_key_offset_boundaries(&self) -> Result<(VarInt, VarInt)>;
}

#[async_trait::async_trait]
impl MonerodClientExt for monerod::Client {
    /// Chooses 10 random key offsets for use within a new confidential
    /// transactions.
    ///
    /// Choosing these offsets randomly is not ideal for privacy, instead they
    /// should be chosen in a way that mimics a real spending pattern as much as
    /// possible.
    async fn calculate_key_offset_boundaries(&self) -> Result<(VarInt, VarInt)> {
        let latest_block = self.get_block_count().await?;
        let latest_spendable_block = latest_block.count - 50;

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

#[cfg(test)]
mod tests {
    use super::*;
    use monero_harness::image::Monerod;
    use monero_rpc::monerod::{Client, GetOutputsOut};
    use testcontainers::clients::Cli;
    use testcontainers::Docker;

    #[tokio::test]
    async fn get_outs_for_key_offsets() {
        let cli = Cli::default();
        let container = cli.run(Monerod::default());
        let rpc_client = Client::localhost(container.get_host_port(18081).unwrap()).unwrap();
        rpc_client.generateblocks(150, "498AVruCDWgP9Az9LjMm89VWjrBrSZ2W2K3HFBiyzzrRjUJWUcCVxvY1iitfuKoek2FdX6MKGAD9Qb1G1P8QgR5jPmmt3Vj".to_owned()).await.unwrap();
        let wallet = Wallet {
            client: rpc_client.clone(),
            key: todo!(),
        };

        let (lower, upper) = wallet.calculate_key_offset_boundaries().await.unwrap();

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
