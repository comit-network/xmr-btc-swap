use crate::clsag::Ring;
use curve25519_dalek::edwards::{CompressedEdwardsY, EdwardsPoint};
use std::borrow::Cow;

macro_rules! hash_to_scalar {
    ($($e:tt) || +) => {
        {
            use crate::macros::ToCowBytes as _;
            use tiny_keccak::Hasher as _;

            let mut hasher = tiny_keccak::Keccak::v256();

            $(
                hasher.update($e.to_cow_bytes().as_ref());
            )+

            let mut hash = [0u8; 32];
            hasher.finalize(&mut hash);

            Scalar::from_bytes_mod_order(hash)
        }
    };
}

type CowBytes<'a> = Cow<'a, [u8]>;

pub(crate) trait ToCowBytes {
    fn to_cow_bytes(&self) -> CowBytes<'_>;
}

impl ToCowBytes for CompressedEdwardsY {
    fn to_cow_bytes(&self) -> CowBytes<'_> {
        CowBytes::Borrowed(self.0.as_ref())
    }
}

impl ToCowBytes for EdwardsPoint {
    fn to_cow_bytes(&self) -> CowBytes<'_> {
        CowBytes::Owned(self.compress().0.to_vec())
    }
}

impl ToCowBytes for Vec<u8> {
    fn to_cow_bytes(&self) -> CowBytes<'_> {
        CowBytes::Borrowed(self.as_ref())
    }
}

impl<const N: usize> ToCowBytes for [u8; N] {
    fn to_cow_bytes(&self) -> CowBytes<'_> {
        CowBytes::Borrowed(self.as_ref())
    }
}

impl ToCowBytes for &[u8] {
    fn to_cow_bytes(&self) -> CowBytes<'_> {
        CowBytes::Borrowed(self)
    }
}

impl<'a> ToCowBytes for Ring<'a> {
    fn to_cow_bytes(&self) -> CowBytes<'_> {
        CowBytes::Borrowed(self.as_ref())
    }
}
