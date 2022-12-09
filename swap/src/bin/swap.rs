#![warn(
    unused_extern_crates,
    missing_copy_implementations,
    rust_2018_idioms,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::fallible_impl_from,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::dbg_macro
)]
#![forbid(unsafe_code)]
#![allow(non_snake_case)]

use anyhow::Result;
use std::env;
use std::sync::Arc;
use swap::cli::command::{parse_args_and_apply_defaults, ParseResult};
use swap::common::check_latest_version;
use tokio::sync::broadcast;

#[tokio::main]
async fn main() -> Result<()> {
    let (tx, mut rx1) = broadcast::channel(1);
    let (context, mut request) = match parse_args_and_apply_defaults(env::args_os(), tx).await? {
        ParseResult::Context(context, request) => (context, request),
        ParseResult::PrintAndExitZero { message } => {
            println!("{}", message);
            std::process::exit(0);
        }
    };

    if let Err(e) = check_latest_version(env!("CARGO_PKG_VERSION")).await {
        eprintln!("{}", e);
    }
    let result = request.call(Arc::clone(&context)).await?;
    println!("{}", result);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::bitcoin::Amount;
    use std::sync::Mutex;
    use std::time::Duration;
    use swap::api::request::determine_btc_to_swap;
    use swap::network::quote::BidQuote;
    use swap::tracing_ext::capture_logs;
    use tracing::level_filters::LevelFilter;

    #[tokio::test]
    async fn given_no_balance_and_transfers_less_than_max_swaps_max_giveable() {
        let writer = capture_logs(LevelFilter::INFO);
        let givable = Arc::new(Mutex::new(MaxGiveable::new(vec![
            Amount::ZERO,
            Amount::from_btc(0.0009).unwrap(),
        ])));

        let (amount, fees) = determine_btc_to_swap(
            true,
            async { Ok(quote_with_max(0.01)) },
            get_dummy_address(),
            || async { Ok(Amount::from_btc(0.001)?) },
            || async {
                let mut result = givable.lock().unwrap();
                result.give()
            },
            || async { Ok(()) },
            |_| async { Ok(Amount::from_sat(1000)) },
        )
        .await
        .unwrap();

        let expected_amount = Amount::from_btc(0.0009).unwrap();
        let expected_fees = Amount::from_btc(0.0001).unwrap();

        assert_eq!((amount, fees), (expected_amount, expected_fees));
        assert_eq!(
            writer.captured(),
            r" INFO swap: Received quote price=0.00100000 BTC minimum_amount=0.00000000 BTC maximum_amount=0.01000000 BTC
 INFO swap: Deposit at least 0.00001000 BTC to cover the min quantity with fee!
 INFO swap: Waiting for Bitcoin deposit deposit_address=1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6 min_deposit=0.00001000 BTC max_giveable=0.00000000 BTC minimum_amount=0.00000000 BTC maximum_amount=0.01000000 BTC
 INFO swap: Received Bitcoin new_balance=0.00100000 BTC max_giveable=0.00090000 BTC
"
        );
    }

    #[tokio::test]
    async fn given_no_balance_and_transfers_more_then_swaps_max_quantity_from_quote() {
        let writer = capture_logs(LevelFilter::INFO);
        let givable = Arc::new(Mutex::new(MaxGiveable::new(vec![
            Amount::ZERO,
            Amount::from_btc(0.1).unwrap(),
        ])));

        let (amount, fees) = determine_btc_to_swap(
            true,
            async { Ok(quote_with_max(0.01)) },
            get_dummy_address(),
            || async { Ok(Amount::from_btc(0.1001)?) },
            || async {
                let mut result = givable.lock().unwrap();
                result.give()
            },
            || async { Ok(()) },
            |_| async { Ok(Amount::from_sat(1000)) },
        )
        .await
        .unwrap();

        let expected_amount = Amount::from_btc(0.01).unwrap();
        let expected_fees = Amount::from_btc(0.0001).unwrap();

        assert_eq!((amount, fees), (expected_amount, expected_fees));
        assert_eq!(
            writer.captured(),
            r" INFO swap: Received quote price=0.00100000 BTC minimum_amount=0.00000000 BTC maximum_amount=0.01000000 BTC
 INFO swap: Deposit at least 0.00001000 BTC to cover the min quantity with fee!
 INFO swap: Waiting for Bitcoin deposit deposit_address=1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6 min_deposit=0.00001000 BTC max_giveable=0.00000000 BTC minimum_amount=0.00000000 BTC maximum_amount=0.01000000 BTC
 INFO swap: Received Bitcoin new_balance=0.10010000 BTC max_giveable=0.10000000 BTC
"
        );
    }

    #[tokio::test]
    async fn given_initial_balance_below_max_quantity_swaps_max_giveable() {
        let writer = capture_logs(LevelFilter::INFO);
        let givable = Arc::new(Mutex::new(MaxGiveable::new(vec![
            Amount::from_btc(0.0049).unwrap(),
            Amount::from_btc(99.9).unwrap(),
        ])));

        let (amount, fees) = determine_btc_to_swap(
            true,
            async { Ok(quote_with_max(0.01)) },
            async { panic!("should not request new address when initial balance  is > 0") },
            || async { Ok(Amount::from_btc(0.005)?) },
            || async {
                let mut result = givable.lock().unwrap();
                result.give()
            },
            || async { Ok(()) },
            |_| async { Ok(Amount::from_sat(1000)) },
        )
        .await
        .unwrap();

        let expected_amount = Amount::from_btc(0.0049).unwrap();
        let expected_fees = Amount::from_btc(0.0001).unwrap();

        assert_eq!((amount, fees), (expected_amount, expected_fees));
        assert_eq!(
            writer.captured(),
            " INFO swap: Received quote price=0.00100000 BTC minimum_amount=0.00000000 BTC maximum_amount=0.01000000 BTC\n"
        );
    }

    #[tokio::test]
    async fn given_initial_balance_above_max_quantity_swaps_max_quantity() {
        let writer = capture_logs(LevelFilter::INFO);
        let givable = Arc::new(Mutex::new(MaxGiveable::new(vec![
            Amount::from_btc(0.1).unwrap(),
            Amount::from_btc(99.9).unwrap(),
        ])));

        let (amount, fees) = determine_btc_to_swap(
            true,
            async { Ok(quote_with_max(0.01)) },
            async { panic!("should not request new address when initial balance is > 0") },
            || async { Ok(Amount::from_btc(0.1001)?) },
            || async {
                let mut result = givable.lock().unwrap();
                result.give()
            },
            || async { Ok(()) },
            |_| async { Ok(Amount::from_sat(1000)) },
        )
        .await
        .unwrap();

        let expected_amount = Amount::from_btc(0.01).unwrap();
        let expected_fees = Amount::from_btc(0.0001).unwrap();

        assert_eq!((amount, fees), (expected_amount, expected_fees));
        assert_eq!(
            writer.captured(),
            " INFO swap: Received quote price=0.00100000 BTC minimum_amount=0.00000000 BTC maximum_amount=0.01000000 BTC\n"
        );
    }

    #[tokio::test]
    async fn given_no_initial_balance_then_min_wait_for_sufficient_deposit() {
        let writer = capture_logs(LevelFilter::INFO);
        let givable = Arc::new(Mutex::new(MaxGiveable::new(vec![
            Amount::ZERO,
            Amount::from_btc(0.01).unwrap(),
        ])));

        let (amount, fees) = determine_btc_to_swap(
            true,
            async { Ok(quote_with_min(0.01)) },
            get_dummy_address(),
            || async { Ok(Amount::from_btc(0.0101)?) },
            || async {
                let mut result = givable.lock().unwrap();
                result.give()
            },
            || async { Ok(()) },
            |_| async { Ok(Amount::from_sat(1000)) },
        )
        .await
        .unwrap();

        let expected_amount = Amount::from_btc(0.01).unwrap();
        let expected_fees = Amount::from_btc(0.0001).unwrap();

        assert_eq!((amount, fees), (expected_amount, expected_fees));
        assert_eq!(
            writer.captured(),
            r" INFO swap: Received quote price=0.00100000 BTC minimum_amount=0.01000000 BTC maximum_amount=184467440737.09551615 BTC
 INFO swap: Deposit at least 0.01001000 BTC to cover the min quantity with fee!
 INFO swap: Waiting for Bitcoin deposit deposit_address=1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6 min_deposit=0.01001000 BTC max_giveable=0.00000000 BTC minimum_amount=0.01000000 BTC maximum_amount=184467440737.09551615 BTC
 INFO swap: Received Bitcoin new_balance=0.01010000 BTC max_giveable=0.01000000 BTC
"
        );
    }

    #[tokio::test]
    async fn given_balance_less_then_min_wait_for_sufficient_deposit() {
        let writer = capture_logs(LevelFilter::INFO);
        let givable = Arc::new(Mutex::new(MaxGiveable::new(vec![
            Amount::from_btc(0.0001).unwrap(),
            Amount::from_btc(0.01).unwrap(),
        ])));

        let (amount, fees) = determine_btc_to_swap(
            true,
            async { Ok(quote_with_min(0.01)) },
            get_dummy_address(),
            || async { Ok(Amount::from_btc(0.0101)?) },
            || async {
                let mut result = givable.lock().unwrap();
                result.give()
            },
            || async { Ok(()) },
            |_| async { Ok(Amount::from_sat(1000)) },
        )
        .await
        .unwrap();

        let expected_amount = Amount::from_btc(0.01).unwrap();
        let expected_fees = Amount::from_btc(0.0001).unwrap();

        assert_eq!((amount, fees), (expected_amount, expected_fees));
        assert_eq!(
            writer.captured(),
            r" INFO swap: Received quote price=0.00100000 BTC minimum_amount=0.01000000 BTC maximum_amount=184467440737.09551615 BTC
 INFO swap: Deposit at least 0.00991000 BTC to cover the min quantity with fee!
 INFO swap: Waiting for Bitcoin deposit deposit_address=1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6 min_deposit=0.00991000 BTC max_giveable=0.00010000 BTC minimum_amount=0.01000000 BTC maximum_amount=184467440737.09551615 BTC
 INFO swap: Received Bitcoin new_balance=0.01010000 BTC max_giveable=0.01000000 BTC
"
        );
    }

    #[tokio::test]
    async fn given_no_initial_balance_and_transfers_less_than_min_keep_waiting() {
        let writer = capture_logs(LevelFilter::INFO);
        let givable = Arc::new(Mutex::new(MaxGiveable::new(vec![
            Amount::ZERO,
            Amount::from_btc(0.01).unwrap(),
            Amount::from_btc(0.01).unwrap(),
            Amount::from_btc(0.01).unwrap(),
            Amount::from_btc(0.01).unwrap(),
        ])));

        let error = tokio::time::timeout(
            Duration::from_secs(1),
            determine_btc_to_swap(
                true,
                async { Ok(quote_with_min(0.1)) },
                get_dummy_address(),
                || async { Ok(Amount::from_btc(0.0101)?) },
                || async {
                    let mut result = givable.lock().unwrap();
                    result.give()
                },
                || async { Ok(()) },
                |_| async { Ok(Amount::from_sat(1000)) },
            ),
        )
        .await
        .unwrap_err();

        assert!(matches!(error, tokio::time::error::Elapsed { .. }));
        assert_eq!(
            writer.captured(),
            r" INFO swap: Received quote price=0.00100000 BTC minimum_amount=0.10000000 BTC maximum_amount=184467440737.09551615 BTC
 INFO swap: Deposit at least 0.10001000 BTC to cover the min quantity with fee!
 INFO swap: Waiting for Bitcoin deposit deposit_address=1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6 min_deposit=0.10001000 BTC max_giveable=0.00000000 BTC minimum_amount=0.10000000 BTC maximum_amount=184467440737.09551615 BTC
 INFO swap: Received Bitcoin new_balance=0.01010000 BTC max_giveable=0.01000000 BTC
 INFO swap: Deposited amount is less than `min_quantity`
 INFO swap: Deposit at least 0.09001000 BTC to cover the min quantity with fee!
 INFO swap: Waiting for Bitcoin deposit deposit_address=1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6 min_deposit=0.09001000 BTC max_giveable=0.01000000 BTC minimum_amount=0.10000000 BTC maximum_amount=184467440737.09551615 BTC
"
        );
    }

    #[tokio::test]
    async fn given_longer_delay_until_deposit_should_not_spam_user() {
        let writer = capture_logs(LevelFilter::INFO);
        let givable = Arc::new(Mutex::new(MaxGiveable::new(vec![
            Amount::ZERO,
            Amount::ZERO,
            Amount::ZERO,
            Amount::ZERO,
            Amount::ZERO,
            Amount::ZERO,
            Amount::ZERO,
            Amount::ZERO,
            Amount::ZERO,
            Amount::from_btc(0.2).unwrap(),
        ])));

        tokio::time::timeout(
            Duration::from_secs(10),
            determine_btc_to_swap(
                true,
                async { Ok(quote_with_min(0.1)) },
                get_dummy_address(),
                || async { Ok(Amount::from_btc(0.21)?) },
                || async {
                    let mut result = givable.lock().unwrap();

                    result.give()
                },
                || async { Ok(()) },
                |_| async { Ok(Amount::from_sat(1000)) },
            ),
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(
            writer.captured(),
            r" INFO swap: Received quote price=0.00100000 BTC minimum_amount=0.10000000 BTC maximum_amount=184467440737.09551615 BTC
 INFO swap: Deposit at least 0.10001000 BTC to cover the min quantity with fee!
 INFO swap: Waiting for Bitcoin deposit deposit_address=1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6 min_deposit=0.10001000 BTC max_giveable=0.00000000 BTC minimum_amount=0.10000000 BTC maximum_amount=184467440737.09551615 BTC
 INFO swap: Received Bitcoin new_balance=0.21000000 BTC max_giveable=0.20000000 BTC
"
        );
    }

    #[tokio::test]
    async fn given_bid_quote_max_amount_0_return_error() {
        let givable = Arc::new(Mutex::new(MaxGiveable::new(vec![
            Amount::from_btc(0.0001).unwrap(),
            Amount::from_btc(0.01).unwrap(),
        ])));

        let determination_error = determine_btc_to_swap(
            true,
            async { Ok(quote_with_max(0.00)) },
            get_dummy_address(),
            || async { Ok(Amount::from_btc(0.0101)?) },
            || async {
                let mut result = givable.lock().unwrap();
                result.give()
            },
            || async { Ok(()) },
            |_| async { Ok(Amount::from_sat(1000)) },
        )
        .await
        .err()
        .unwrap()
        .to_string();

        assert_eq!("Received quote of 0", determination_error);
    }

    struct MaxGiveable {
        amounts: Vec<Amount>,
        call_counter: usize,
    }

    impl MaxGiveable {
        fn new(amounts: Vec<Amount>) -> Self {
            Self {
                amounts,
                call_counter: 0,
            }
        }
        fn give(&mut self) -> Result<Amount> {
            let amount = self
                .amounts
                .get(self.call_counter)
                .ok_or_else(|| anyhow::anyhow!("No more balances available"))?;
            self.call_counter += 1;
            Ok(*amount)
        }
    }

    fn quote_with_max(btc: f64) -> BidQuote {
        BidQuote {
            price: Amount::from_btc(0.001).unwrap(),
            max_quantity: Amount::from_btc(btc).unwrap(),
            min_quantity: Amount::ZERO,
        }
    }

    fn quote_with_min(btc: f64) -> BidQuote {
        BidQuote {
            price: Amount::from_btc(0.001).unwrap(),
            max_quantity: Amount::max_value(),
            min_quantity: Amount::from_btc(btc).unwrap(),
        }
    }

    async fn get_dummy_address() -> Result<bitcoin::Address> {
        Ok("1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6".parse()?)
    }
}
