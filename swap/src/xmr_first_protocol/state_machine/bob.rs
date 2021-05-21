use std::collections::VecDeque;

pub struct StateMachine {
    state: State,
    actions: VecDeque<Action>,
    events: VecDeque<Event>,
}

impl StateMachine {
    fn inject_event(&mut self, event: Event) {
        match self.state {
            State::WatchingForXmrLock => match event {
                Event::XmrConfirmed => {
                    self.actions.push_back(Action::SignAndBroadcastBtcLock);
                    self.state = State::WaitingForBtcRedeem;
                }
                Event::T1Elapsed => {
                    self.state = State::Aborted;
                }
                Event::XmrRefundSeenInMempool => {
                    self.state = State::Aborted;
                }
                _ => panic!("unhandled scenario"),
            },
            State::WaitingForBtcRedeem => match event {
                Event::BtcRedeemSeenInMempool => {
                    self.actions.push_back(Action::BroadcastXmrRedeem);
                    self.state = State::Success;
                }
                Event::T1Elapsed => {
                    self.actions.push_back(Action::SignAndBroadcastBtcRefund);
                    self.state = State::Refunded;
                }
                Event::XmrRefundSeenInMempool => {
                    self.actions.push_back(Action::SignAndBroadcastBtcRefund);
                    self.state = State::Refunded;
                }
                _ => panic!("unhandled scenario"),
            },
            _ => {}
        }
    }

    fn poll(&mut self) -> Poll<Action> {
        if let Some(action) = self.actions.pop_front() {
            Poll::Ready(action)
        } else {
            Poll::Pending
        }
    }
}

#[derive(PartialEq, Debug)]
pub enum State {
    WatchingForXmrLock,
    WaitingForBtcRedeem,
    Success,
    Refunded,
    Aborted,
}

pub enum Event {
    XmrConfirmed,
    // This will contain the s_a allowing bob to build xmr_redeem
    BtcRedeemSeenInMempool,
    XmrRefundSeenInMempool,
    T1Elapsed,
}

pub enum Action {
    SignAndBroadcastBtcLock,
    BroadcastXmrRedeem,
    SignAndBroadcastBtcRefund,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path() {
        let mut state_machine = StateMachine {
            state: State::WatchingForXmrLock,
            actions: Default::default(),
            events: Default::default(),
        };
        state_machine.events.push_back(Event::XmrConfirmed);
        state_machine
            .events
            .push_back(Event::BtcRedeemSeenInMempool);
        state_machine.run();
        assert_eq!(state_machine.state, State::Success);
    }

    #[test]
    fn alice_fails_to_redeem_btc_before_t1() {
        let mut state_machine = StateMachine {
            state: State::WatchingForXmrLock,
            actions: Default::default(),
            events: Default::default(),
        };
        state_machine.events.push_back(Event::XmrConfirmed);
        state_machine.events.push_back(Event::T1Elapsed);
        state_machine.run();
        assert_eq!(state_machine.state, State::Refunded);
    }

    #[test]
    fn alice_tries_to_refund_xmr_after_redeeming_btc() {
        let mut state_machine = StateMachine {
            state: State::WatchingForXmrLock,
            actions: Default::default(),
            events: Default::default(),
        };
        state_machine.events.push_back(Event::XmrConfirmed);
        state_machine.events.push_back(Event::T1Elapsed);
        state_machine.run();
        assert_eq!(state_machine.state, State::Refunded);
    }
}
