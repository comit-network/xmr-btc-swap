use proptest::prelude::*;

pub mod ecdsa_fun {
    use super::*;
    use ::ecdsa_fun::fun::marker::{Mark, NonZero, Normal};
    use ::ecdsa_fun::fun::{Point, Scalar, G};

    pub fn point() -> impl Strategy<Value = Point> {
        scalar().prop_map(|mut scalar| Point::from_scalar_mul(&G, &mut scalar).mark::<Normal>())
    }

    pub fn scalar() -> impl Strategy<Value = Scalar> {
        prop::array::uniform32(0..255u8).prop_filter_map("generated the 0 element", |bytes| {
            Scalar::from_bytes_mod_order(bytes).mark::<NonZero>()
        })
    }
}

pub mod bitcoin {
    use super::*;
    use ::bitcoin::util::bip32::ExtendedPrivKey;
    use ::bitcoin::Network;

    pub fn extended_priv_key() -> impl Strategy<Value = ExtendedPrivKey> {
        prop::array::uniform8(0..255u8).prop_filter_map("invalid secret key generated", |bytes| {
            ExtendedPrivKey::new_master(Network::Regtest, &bytes).ok()
        })
    }
}
