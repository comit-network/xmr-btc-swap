#![warn(
    unused_extern_crates,
    missing_debug_implementations,
    missing_copy_implementations,
    rust_2018_idioms,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::fallible_impl_from,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::dbg_macro
)]
#![cfg_attr(not(test), warn(clippy::unwrap_used))]
#![forbid(unsafe_code)]
#![allow(non_snake_case)]

#[derive(Debug, Clone, Copy)]
pub enum Epoch {
    T0,
    T1,
    T2,
}

#[macro_use]
mod utils {

    macro_rules! impl_try_from_parent_enum {
        ($type:ident, $parent:ident) => {
            impl TryFrom<$parent> for $type {
                type Error = anyhow::Error;
                fn try_from(from: $parent) -> Result<Self> {
                    if let $parent::$type(inner) = from {
                        Ok(inner)
                    } else {
                        Err(anyhow::anyhow!(
                            "Failed to convert parent state to child state"
                        ))
                    }
                }
            }
        };
    }

    macro_rules! impl_from_child_enum {
        ($type:ident, $parent:ident) => {
            impl From<$type> for $parent {
                fn from(from: $type) -> Self {
                    $parent::$type(from)
                }
            }
        };
    }
}

pub mod alice;
pub mod bitcoin;
pub mod bob;
pub mod config;
pub mod monero;
pub mod serde;
pub mod transport;

pub use cross_curve_dleq;
