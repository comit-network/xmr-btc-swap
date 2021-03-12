use crate::ui::model::bitcoin;
use crate::ui::model::swap::State;
use crate::ui::model::swap_amounts::SwapAmounts;
use druid::widget::Label;
use druid::{Env, Widget};

pub fn swap_amounts() -> impl Widget<SwapAmounts> {
    Label::new(|data: &SwapAmounts, _: &Env| format!("{}", data))
}

pub fn bitcoin_balance() -> impl Widget<bitcoin::Amount> {
    Label::new(|data: &bitcoin::Amount, _: &Env| format!("{} BTC", data))
}

pub fn swap_state() -> impl Widget<State> {
    Label::new(|data: &State, _: &Env| format!("{}", data))
}
