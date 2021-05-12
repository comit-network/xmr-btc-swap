use curve25519_dalek::edwards::{CompressedEdwardsY, EdwardsPoint};
use std::borrow::Cow;

macro_rules! hash_to_scalar {
    ($($e:tt) || +) => {
        {
            use crate::macros::ToCowBytes as _;
            use tiny_keccak::Hasher as _;

            let mut hasher = tiny_keccak::Keccak::v256();

            $(
                let bytes_vec = $e.to_cow_bytes();

                for el in bytes_vec {
                    hasher.update(el.as_ref());
                }
            )+

            let mut hash = [0u8; 32];
            hasher.finalize(&mut hash);

            Scalar::from_bytes_mod_order(hash)
        }
    };
}

type CowBytes<'a> = Cow<'a, [u8; 32]>;

pub(crate) trait ToCowBytes {
    fn to_cow_bytes(&self) -> Vec<CowBytes<'_>>;
}

impl ToCowBytes for CompressedEdwardsY {
    fn to_cow_bytes(&self) -> Vec<CowBytes<'_>> {
        vec![CowBytes::Borrowed(&self.0)]
    }
}

impl ToCowBytes for EdwardsPoint {
    fn to_cow_bytes(&self) -> Vec<CowBytes<'_>> {
        vec![CowBytes::Owned(self.compress().0)]
    }
}

impl ToCowBytes for [u8; 32] {
    fn to_cow_bytes(&self) -> Vec<CowBytes<'_>> {
        vec![CowBytes::Borrowed(&self)]
    }
}

impl ToCowBytes for [u8; 11] {
    fn to_cow_bytes(&self) -> Vec<CowBytes<'_>> {
        let mut bytes = [0u8; 32];
        bytes[0..11].copy_from_slice(self);

        vec![CowBytes::Owned(bytes)]
    }
}

impl<'a> ToCowBytes for [EdwardsPoint; 11] {
    fn to_cow_bytes(&self) -> Vec<CowBytes<'_>> {
        vec![
            CowBytes::Owned(self[0].compress().0),
            CowBytes::Owned(self[1].compress().0),
            CowBytes::Owned(self[2].compress().0),
            CowBytes::Owned(self[3].compress().0),
            CowBytes::Owned(self[4].compress().0),
            CowBytes::Owned(self[5].compress().0),
            CowBytes::Owned(self[6].compress().0),
            CowBytes::Owned(self[7].compress().0),
            CowBytes::Owned(self[8].compress().0),
            CowBytes::Owned(self[9].compress().0),
            CowBytes::Owned(self[10].compress().0),
        ]
    }
}
