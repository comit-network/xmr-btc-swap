use anyhow::{anyhow, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::path::Path;

pub struct Database<T>
where
    T: Serialize + DeserializeOwned,
{
    db: sled::Db,
    _marker: std::marker::PhantomData<T>,
}

impl<T> Database<T>
where
    T: Serialize + DeserializeOwned,
{
    const LAST_STATE_KEY: &'static str = "latest_state";

    pub fn open(path: &Path) -> Result<Self> {
        let db =
            sled::open(path).with_context(|| format!("Could not open the DB at {:?}", path))?;

        Ok(Database {
            db,
            _marker: Default::default(),
        })
    }

    pub async fn insert_latest_state(&self, state: &T) -> Result<()> {
        let key = serialize(&Self::LAST_STATE_KEY)?;
        let new_value = serialize(&state).context("Could not serialize new state value")?;

        let old_value = self.db.get(&key)?;

        self.db
            .compare_and_swap(key, old_value, Some(new_value))
            .context("Could not write in the DB")?
            .context("Stored swap somehow changed, aborting saving")?;

        self.db
            .flush_async()
            .await
            .map(|_| ())
            .context("Could not flush db")
    }

    pub fn get_latest_state(&self) -> anyhow::Result<T> {
        let key = serialize(&Self::LAST_STATE_KEY)?;

        let encoded = self
            .db
            .get(&key)?
            .ok_or_else(|| anyhow!("State does not exist {:?}", key))?;

        let state = deserialize(&encoded).context("Could not deserialize state")?;
        Ok(state)
    }
}

pub fn serialize<T>(t: &T) -> anyhow::Result<Vec<u8>>
where
    T: Serialize,
{
    Ok(serde_cbor::to_vec(t)?)
}

pub fn deserialize<T>(v: &[u8]) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    Ok(serde_cbor::from_slice(&v)?)
}

#[cfg(test)]
mod tests {
    #![allow(non_snake_case)]
    use super::*;
    use bitcoin::SigHash;
    use curve25519_dalek::scalar::Scalar;
    use ecdsa_fun::fun::rand_core::OsRng;
    use std::str::FromStr;
    use xmr_btc::serde::{bitcoin_amount, cross_curve_dleq_scalar, monero_private_key};

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    pub struct TestState {
        A: xmr_btc::bitcoin::PublicKey,
        a: xmr_btc::bitcoin::SecretKey,
        #[serde(with = "cross_curve_dleq_scalar")]
        s_a: ::cross_curve_dleq::Scalar,
        #[serde(with = "monero_private_key")]
        s_b: monero::PrivateKey,
        S_a_monero: ::monero::PublicKey,
        S_a_bitcoin: xmr_btc::bitcoin::PublicKey,
        v: xmr_btc::monero::PrivateViewKey,
        #[serde(with = "bitcoin_amount")]
        btc: ::bitcoin::Amount,
        xmr: xmr_btc::monero::Amount,
        refund_timelock: u32,
        refund_address: ::bitcoin::Address,
        transaction: ::bitcoin::Transaction,
        tx_punish_sig: xmr_btc::bitcoin::Signature,
    }

    #[tokio::test]
    async fn recover_state_from_db() {
        let db = Database::open(Path::new("../target/test_recover.db")).unwrap();

        let a = xmr_btc::bitcoin::SecretKey::new_random(&mut OsRng);
        let s_a = cross_curve_dleq::Scalar::random(&mut OsRng);
        let s_b = monero::PrivateKey::from_scalar(Scalar::random(&mut OsRng));
        let v_a = xmr_btc::monero::PrivateViewKey::new_random(&mut OsRng);
        let S_a_monero = monero::PublicKey::from_private_key(&monero::PrivateKey {
            scalar: s_a.into_ed25519(),
        });
        let S_a_bitcoin = s_a.into_secp256k1().into();
        let tx_punish_sig = a.sign(SigHash::default());

        let state = TestState {
            A: a.public(),
            a,
            s_b,
            s_a,
            S_a_monero,
            S_a_bitcoin,
            v: v_a,
            btc: ::bitcoin::Amount::from_sat(100),
            xmr: xmr_btc::monero::Amount::from_piconero(1000),
            refund_timelock: 0,
            refund_address: ::bitcoin::Address::from_str("1L5wSMgerhHg8GZGcsNmAx5EXMRXSKR3He")
                .unwrap(),
            transaction: ::bitcoin::Transaction {
                version: 0,
                lock_time: 0,
                input: vec![::bitcoin::TxIn::default()],
                output: vec![::bitcoin::TxOut::default()],
            },
            tx_punish_sig,
        };

        db.insert_latest_state(&state)
            .await
            .expect("Failed to save state the first time");
        let recovered: TestState = db
            .get_latest_state()
            .expect("Failed to recover state the first time");

        // We insert and recover twice to ensure database implementation allows the
        // caller to write to an existing key
        db.insert_latest_state(&recovered)
            .await
            .expect("Failed to save state the second time");
        let recovered: TestState = db
            .get_latest_state()
            .expect("Failed to recover state the second time");

        assert_eq!(state, recovered);
    }
}
