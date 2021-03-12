pub mod swap_amounts {
    use crate::{bitcoin, monero};
    use druid::Data;
    use std::fmt;
    use std::fmt::{Display, Formatter};

    #[derive(Copy, Clone, Data)]
    pub struct SwapAmounts {
        #[data(same_fn = "PartialEq::eq")]
        pub bitcoin: bitcoin::Amount,
        #[data(same_fn = "PartialEq::eq")]
        pub monero: monero::Amount,
    }

    impl Display for SwapAmounts {
        fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
            write!(f, "Swapping {} for {}", self.bitcoin, self.monero)
        }
    }
}

pub mod bitcoin {

    use bitcoin::hashes::core::fmt::Formatter;
    use druid::Data;
    use std::fmt::Display;
    use std::{fmt, ops};

    #[derive(Copy, Clone, Data)]
    pub struct Amount(#[data(same_fn = "PartialEq::eq")] ::bitcoin::Amount);

    impl Amount {
        pub fn from_btc(amount: f64) -> Self {
            Self {
                0: ::bitcoin::Amount::from_btc(amount).expect("this is only used for testing"),
            }
        }

        pub fn zero() -> Self {
            Self {
                0: ::bitcoin::Amount::ZERO,
            }
        }
    }

    impl Display for Amount {
        fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.0.as_btc())?;
            Ok(())
        }
    }

    impl ops::Add for Amount {
        type Output = Amount;

        fn add(self, rhs: Self) -> Self::Output {
            Self {
                0: self.0.add(rhs.0),
            }
        }
    }
}

pub mod swap {
    use crate::protocol::bob::BobState;
    use druid::Data;
    use std::fmt;
    use std::fmt::{Display, Formatter};

    pub const NOT_TRIGGERED: &str = "swap-not-triggered";
    pub const TRIGGERED: &str = "swap-triggered";
    pub const RUNNING: &str = "swap-running";

    pub const EXEC: &str = "swap-exec";

    #[derive(Clone, Data, PartialEq)]
    pub struct State {
        #[data(same_fn = "PartialEq::eq")]
        key: String,
        label: String,
    }

    impl State {
        pub fn not_triggered() -> Self {
            Self {
                key: NOT_TRIGGERED.to_string(),
                label: "Press the \"swap\" button to trigger a swap".to_string(),
            }
        }

        pub fn triggered() -> Self {
            Self {
                key: TRIGGERED.to_string(),
                label: "Starting swap...".to_string(),
            }
        }

        pub fn running() -> Self {
            Self {
                key: RUNNING.to_string(),
                label: "Swap ongoing".to_string(),
            }
        }

        pub fn none() -> Self {
            Self {
                key: format!("{}{}", EXEC, "none"),
                label: "".to_string(),
            }
        }

        pub fn insufficient_btc() -> Self {
            Self {
                key: format!("{}{}", EXEC, "insufficient-funds"),
                label: "Insufficient BTC to do the swap".to_string(),
            }
        }
    }

    impl From<BobState> for State {
        fn from(state: BobState) -> Self {
            match state {
                BobState::Started { .. } => State {
                    key: format!("{}{}", EXEC, state),
                    label: format!("Swap currently in state: {}", state),
                },
                BobState::ExecutionSetupDone(_) => State {
                    key: format!("{}{}", EXEC, state),
                    label: format!("Swap currently in state: {}", state),
                },
                BobState::BtcLocked(_) => State {
                    key: format!("{}{}", EXEC, state),
                    label: format!("Swap currently in state: {}", state),
                },
                BobState::XmrLockProofReceived { .. } => State {
                    key: format!("{}{}", EXEC, state),
                    label: format!("Swap currently in state: {}", state),
                },
                BobState::XmrLocked(_) => State {
                    key: format!("{}{}", EXEC, state),
                    label: format!("Swap currently in state: {}", state),
                },
                BobState::EncSigSent(_) => State {
                    key: format!("{}{}", EXEC, state),
                    label: format!("Swap currently in state: {}", state),
                },
                BobState::BtcRedeemed(_) => State {
                    key: format!("{}{}", EXEC, state),
                    label: format!("Swap currently in state: {}", state),
                },
                BobState::CancelTimelockExpired(_) => State {
                    key: format!("{}{}", EXEC, state),
                    label: format!("Swap currently in state: {}", state),
                },
                BobState::BtcCancelled(_) => State {
                    key: format!("{}{}", EXEC, state),
                    label: format!("Swap currently in state: {}", state),
                },
                BobState::BtcRefunded(_) => State {
                    key: format!("{}{}", EXEC, state),
                    label: format!("Swap currently in state: {}", state),
                },
                BobState::XmrRedeemed { .. } => State {
                    key: format!("{}{}", EXEC, state),
                    label: format!("Swap currently in state: {}", state),
                },
                BobState::BtcPunished { .. } => State {
                    key: format!("{}{}", EXEC, state),
                    label: format!("Swap currently in state: {}", state),
                },
                BobState::SafelyAborted => State {
                    key: format!("{}{}", EXEC, state),
                    label: format!("Swap currently in state: {}", state),
                },
            }
        }
    }

    impl Display for State {
        fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
            self.label.fmt(f)
        }
    }
}
