use proptest::prelude::*;

pub mod ecdsa_fun {
    use super::*;
    use ::ecdsa_fun::fun::{Point, Scalar, G};

    pub fn point() -> impl Strategy<Value = Point> {
        scalar().prop_map(|mut scalar| Point::even_y_from_scalar_mul(G, &mut scalar).normalize())
    }

    pub fn scalar() -> impl Strategy<Value = Scalar> {
        prop::array::uniform32(0..255u8).prop_filter_map("generated the 0 element", |bytes| {
            Scalar::from_bytes_mod_order(bytes).non_zero()
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
