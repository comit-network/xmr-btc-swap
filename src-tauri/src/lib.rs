use std::result::Result;
use std::sync::Arc;
use swap::{
    api::{
        request::{
            get_balance as get_balance_impl, get_swap_infos_all as get_swap_infos_all_impl,
            withdraw_btc as withdraw_btc_impl, BalanceArgs, BalanceResponse, GetSwapInfoResponse,
            WithdrawBtcArgs, WithdrawBtcResponse,
        },
        Context,
    },
    cli::command::{Bitcoin, Monero},
};
use tauri::{Emitter, Manager, State};

trait ToStringResult<T> {
    fn to_string_result(self) -> Result<T, String>;
}

// Implement the trait for Result<T, E>
impl<T, E: ToString> ToStringResult<T> for Result<T, E> {
    fn to_string_result(self) -> Result<T, String> {
        match self {
            Ok(value) => Ok(value),
            Err(err) => Err(err.to_string()),
        }
    }
}

#[tauri::command]
async fn get_balance(context: State<'_, Arc<Context>>) -> Result<BalanceResponse, String> {
    get_balance_impl(
        BalanceArgs {
            force_refresh: true,
        },
        context.inner().clone(),
    )
    .await
    .to_string_result()
}

#[tauri::command]
async fn get_swap_infos_all(
    context: State<'_, Arc<Context>>,
) -> Result<Vec<GetSwapInfoResponse>, String> {
    get_swap_infos_all_impl(context.inner().clone())
        .await
        .to_string_result()
}

/*macro_rules! tauri_command {
    ($command_name:ident, $command_args:ident, $command_response:ident) => {
        #[tauri::command]
        async fn $command_name(
            context: State<'_, Context>,
            args: $command_args,
        ) -> Result<$command_response, String> {
            swap::api::request::$command_name(args, context)
                .await
                .to_string_result()
        }
    };
}*/

#[tauri::command]
async fn withdraw_btc(
    context: State<'_, Arc<Context>>,
    args: WithdrawBtcArgs,
) -> Result<WithdrawBtcResponse, String> {
    withdraw_btc_impl(args, context.inner().clone())
        .await
        .to_string_result()
}

fn setup<'a>(app: &'a mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    tauri::async_runtime::block_on(async {
        let context = Context::build(
            Some(Bitcoin {
                bitcoin_electrum_rpc_url: None,
                bitcoin_target_block: None,
            }),
            Some(Monero {
                monero_daemon_address: None,
            }),
            None,
            None,
            true,
            true,
            true,
            None,
        )
        .await
        .unwrap()
        .with_tauri_handle(app.app_handle().to_owned());

        app.manage(Arc::new(context));
    });

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_balance,
            get_swap_infos_all,
            withdraw_btc
        ])
        .setup(setup)
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
