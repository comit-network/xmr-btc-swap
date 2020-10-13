pub mod node;
pub mod storage;
pub mod transport;
pub mod wallet;

pub mod bob {
    use xmr_btc::bob::State;

    // TODO: use macro or generics
    pub fn is_state5(state: &State) -> bool {
        matches!(state, State::State5 { .. })
    }

    // TODO: use macro or generics
    pub fn is_state3(state: &State) -> bool {
        matches!(state, State::State3 { .. })
    }
}

pub mod alice {
    use xmr_btc::alice::State;

    // TODO: use macro or generics
    pub fn is_state4(state: &State) -> bool {
        matches!(state, State::State4 { .. })
    }

    // TODO: use macro or generics
    pub fn is_state5(state: &State) -> bool {
        matches!(state, State::State5 { .. })
    }

    // TODO: use macro or generics
    pub fn is_state6(state: &State) -> bool {
        matches!(state, State::State6 { .. })
    }
}
