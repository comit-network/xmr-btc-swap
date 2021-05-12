use std::collections::VecDeque;
use std::task::Poll;

pub struct StateMachine {
    state: State,
    actions: VecDeque<Action>,
    events: VecDeque<Event>,
}

impl StateMachine {
    fn inject_event(&mut self, event: Event) {
        match self.state {
            State::WatchingForBtcLock => match event {
                Event::BtcLockSeenInMempool => {
                    self.actions.push_back(Action::SignAndBroadcastBtcRedeem);
                    self.actions.push_back(Action::WatchForXmrRedeem);
                    self.state = State::WatchingForXmrRedeem;
                }
                Event::BtcLockTimeoutElapsed => {
                    self.actions.push_back(Action::BroadcastXmrRefund);
                    self.state = State::Aborted;
                }
                _ => {}
            },
            State::WatchingForXmrRedeem => match event {
                Event::T2Elapsed => {
                    self.actions.push_back(Action::BroadcastXmrRefund);
                    self.actions.push_back(Action::SignAndBroadcastBtcPunish);
                    self.state = State::Punished;
                }
                Event::XmrRedeemSeenInMempool => {
                    self.actions.push_back(Action::SignAndBroadcastBtcPunish);
                    self.state = State::Success;
                }
                _ => {}
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
    WatchingForBtcLock,
    WatchingForXmrRedeem,
    Punished,
    Success,
    Aborted,
}

pub enum Event {
    BtcLockSeenInMempool,
    T2Elapsed,
    BtcLockTimeoutElapsed,
    XmrRedeemSeenInMempool,
}

// These actions should not fail (are retried until successful) and should be
// idempotent This allows us to greatly simplify the state machine
pub enum Action {
    WatchForXmrRedeem,
    SignAndBroadcastBtcPunish,
    SignAndBroadcastBtcRedeem,
    BroadcastXmrRefund,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path() {
        let mut state_machine = StateMachine {
            state: State::WatchingForBtcLock,
            actions: Default::default(),
            events: Default::default(),
        };
        state_machine.inject_event(Event::BtcLockSeenInMempool);
        state_machine.inject_event(Event::XmrRedeemSeenInMempool);
        assert_eq!(state_machine.state, State::Success);
    }

    #[test]
    fn bob_fails_to_lock_btc() {
        let mut state_machine = StateMachine {
            state: State::WatchingForBtcLock,
            actions: Default::default(),
            events: Default::default(),
        };
        state_machine.events.push_back(Event::BtcLockTimeoutElapsed);
        state_machine.poll();
        assert_eq!(state_machine.state, State::Aborted);
    }

    #[test]
    fn bob_fails_to_redeem_xmr_before_t2() {
        let mut state_machine = StateMachine {
            state: State::WatchingForBtcLock,
            actions: Default::default(),
            events: Default::default(),
        };
        state_machine.events.push_back(Event::BtcLockSeenInMempool);
        state_machine.events.push_back(Event::T2Elapsed);
        state_machine.run();
        assert_eq!(state_machine.state, State::Punished);
    }
}
