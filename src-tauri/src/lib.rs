use std::sync::Arc;

use once_cell::sync::OnceCell;
use std::result::Result;
use swap::{
    api::{
        request::{
            get_balance as get_balance_impl, get_swap_infos_all as get_swap_infos_all_impl,
            BalanceArgs, BalanceResponse, GetSwapInfoResponse,
        },
        Context,
    },
    cli::command::{Bitcoin, Monero},
};

// Lazy load the Context
static CONTEXT: OnceCell<Arc<Context>> = OnceCell::new();

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
async fn get_balance() -> Result<BalanceResponse, String> {
    let context = CONTEXT.get().unwrap();

    get_balance_impl(
        BalanceArgs {
            force_refresh: true,
        },
        context.clone(),
    )
    .await
    .to_string_result()
}

#[tauri::command]
async fn get_swap_infos_all() -> Result<Vec<GetSwapInfoResponse>, String> {
    let context = CONTEXT.get().unwrap();

    get_swap_infos_all_impl(context.clone())
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
        .unwrap();

        CONTEXT
            .set(Arc::new(context))
            .expect("Failed to initialize cli context");
    });

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_balance, get_swap_infos_all])
        .setup(setup)
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
