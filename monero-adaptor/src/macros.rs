use crate::ring::Ring;
use curve25519_dalek::edwards::CompressedEdwardsY;

macro_rules! hash_to_scalar {
    ($($e:tt) || +) => {
        {
            use crate::macros::AsByteSlice as _;
            use tiny_keccak::Hasher as _;

            let mut hasher = tiny_keccak::Keccak::v256();

            $(
                hasher.update($e.as_byte_slice());
            )+

            let mut hash = [0u8; 32];
            hasher.finalize(&mut hash);

            Scalar::from_bytes_mod_order(hash)
        }
    };
}

pub(crate) trait AsByteSlice {
    fn as_byte_slice(&self) -> &[u8];
}

impl AsByteSlice for CompressedEdwardsY {
    fn as_byte_slice(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl AsByteSlice for Vec<u8> {
    fn as_byte_slice(&self) -> &[u8] {
        self.as_ref()
    }
}

impl<const N: usize> AsByteSlice for [u8; N] {
    fn as_byte_slice(&self) -> &[u8] {
        self.as_ref()
    }
}

impl AsByteSlice for &[u8] {
    fn as_byte_slice(&self) -> &[u8] {
        self
    }
}

impl<'a> AsByteSlice for Ring<'a> {
    fn as_byte_slice(&self) -> &[u8] {
        self.as_ref()
    }
}
