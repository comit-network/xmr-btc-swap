use crate::bitcoin::Scalar;
use ecdsa_fun::fun::marker::{NonZero, Secret};

pub trait ScalarExt {
    fn to_secpfun_scalar(&self) -> ecdsa_fun::fun::Scalar;
}

impl ScalarExt for crate::monero::Scalar {
    fn to_secpfun_scalar(&self) -> Scalar<Secret, NonZero> {
        let mut little_endian_bytes = self.to_bytes();

        little_endian_bytes.reverse();
        let big_endian_bytes = little_endian_bytes;

        ecdsa_fun::fun::Scalar::from_bytes(big_endian_bytes)
            .expect("valid scalar")
            .non_zero()
            .expect("non-zero scalar")
    }
}
