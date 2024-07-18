use std::sync::Arc;

use once_cell::sync::OnceCell;
use swap::{api::{request::{Method, Request}, Context}, cli::command::{Bitcoin, Monero}};

// Lazy load the Context
static CONTEXT: OnceCell<Arc<Context>> = OnceCell::new();

#[tauri::command]
async fn balance() -> String {
    let context = CONTEXT.get().unwrap();
    let request = Request::new(Method::Balance { force_refresh: true });
    let response = request.call(context.clone()).await.unwrap();
    response.to_string()
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

        CONTEXT.set(Arc::new(context)).expect("Failed to initialize cli context");
    });
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![balance])
        .setup(setup)
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
