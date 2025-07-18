use std::collections::HashMap;
use std::io::Write;
use std::result::Result;
use std::sync::Arc;
use swap::cli::{
    api::{
        data,
        request::{
            BalanceArgs, BuyXmrArgs, CancelAndRefundArgs, CheckElectrumNodeArgs,
            CheckElectrumNodeResponse, CheckMoneroNodeArgs, CheckMoneroNodeResponse, CheckSeedArgs,
            CheckSeedResponse, ExportBitcoinWalletArgs, GetCurrentSwapArgs, GetDataDirArgs,
            GetHistoryArgs, GetLogsArgs, GetMoneroAddressesArgs, GetMoneroBalanceArgs,
            GetMoneroHistoryArgs, GetMoneroMainAddressArgs, GetMoneroSyncProgressArgs,
            GetPendingApprovalsResponse, GetRestoreHeightArgs, GetSwapInfoArgs,
            GetSwapInfosAllArgs, ListSellersArgs, MoneroRecoveryArgs, RedactArgs,
            RejectApprovalArgs, RejectApprovalResponse, ResolveApprovalArgs, ResumeSwapArgs,
            SendMoneroArgs, SetRestoreHeightArgs, SuspendCurrentSwapArgs, WithdrawBtcArgs,
        },
        tauri_bindings::{TauriContextStatusEvent, TauriEmitter, TauriHandle, TauriSettings},
        Context, ContextBuilder,
    },
    command::Bitcoin,
};
use tauri::{async_runtime::RwLock, Manager, RunEvent};
use tauri_plugin_dialog::DialogExt;
use zip::{write::SimpleFileOptions, ZipWriter};

/// Trait to convert Result<T, E> to Result<T, String>
/// Tauri commands require the error type to be a string
trait ToStringResult<T> {
    fn to_string_result(self) -> Result<T, String>;
}

impl<T, E: ToString> ToStringResult<T> for Result<T, E> {
    fn to_string_result(self) -> Result<T, String> {
        self.map_err(|e| e.to_string())
    }
}

/// This macro is used to create boilerplate functions as tauri commands
/// that simply delegate handling to the respective request type.
///
/// # Example
/// ```ignored
/// tauri_command!(get_balance, BalanceArgs);
/// ```
/// will resolve to
/// ```ignored
/// #[tauri::command]
/// async fn get_balance(context: tauri::State<'...>, args: BalanceArgs) -> Result<BalanceArgs::Response, String> {
///     args.handle(context.inner().clone()).await.to_string_result()
/// }
/// ```
/// # Example 2
/// ```ignored
/// tauri_command!(get_balance, BalanceArgs, no_args);
/// ```
/// will resolve to
/// ```ignored
/// #[tauri::command]
/// async fn get_balance(context: tauri::State<'...>) -> Result<BalanceArgs::Response, String> {
///    BalanceArgs {}.handle(context.inner().clone()).await.to_string_result()
/// }
/// ```
macro_rules! tauri_command {
    ($fn_name:ident, $request_name:ident) => {
        #[tauri::command]
        async fn $fn_name(
            state: tauri::State<'_, State>,
            args: $request_name,
        ) -> Result<<$request_name as swap::cli::api::request::Request>::Response, String> {
            // Throw error if context is not available
            let context = state.try_get_context()?;

            <$request_name as swap::cli::api::request::Request>::request(args, context)
                .await
                .to_string_result()
        }
    };
    ($fn_name:ident, $request_name:ident, no_args) => {
        #[tauri::command]
        async fn $fn_name(
            state: tauri::State<'_, State>,
        ) -> Result<<$request_name as swap::cli::api::request::Request>::Response, String> {
            // Throw error if context is not available
            let context = state.try_get_context()?;

            <$request_name as swap::cli::api::request::Request>::request($request_name {}, context)
                .await
                .to_string_result()
        }
    };
}

/// Represents the shared Tauri state. It is accessed by Tauri commands
struct State {
    pub context: RwLock<Option<Arc<Context>>>,
    pub handle: TauriHandle,
}

impl State {
    /// Creates a new State instance with no Context
    fn new(handle: TauriHandle) -> Self {
        Self {
            context: RwLock::new(None),
            handle,
        }
    }

    /// Attempts to retrieve the context
    /// Returns an error if the context is not available
    fn try_get_context(&self) -> Result<Arc<Context>, String> {
        self.context
            .try_read()
            .map_err(|_| "Context is being modified".to_string())?
            .clone()
            .ok_or("Context not available".to_string())
    }
}

/// Sets up the Tauri application
/// Initializes the Tauri state
/// Sets the window title
fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Set the window title to include the window title and version
    let config = app.config();
    let title = format!(
        "{} (v{})",
        config
            .app
            .windows
            .first()
            .expect("Window object to be set in config")
            .title
            .as_str(),
        config.version.as_ref().expect("Version to be set")
    );

    let _ = app
        .get_webview_window("main")
        .expect("main window to exist")
        .set_title(&title);

    let app_handle = app.app_handle().to_owned();

    // We need to set a value for the Tauri state right at the start
    // If we don't do this, Tauri commands will panic at runtime if no value is present
    let handle = TauriHandle::new(app_handle.clone());
    let state = State::new(handle);
    app_handle.manage::<State>(state);

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
            let _ = app
                .get_webview_window("main")
                .expect("no main window")
                .set_focus();
        }));

        builder = builder.plugin(tauri_plugin_cli::init());
    }

    builder
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_balance,
            get_monero_addresses,
            get_swap_info,
            get_swap_infos_all,
            withdraw_btc,
            buy_xmr,
            resume_swap,
            get_history,
            monero_recovery,
            get_logs,
            list_sellers,
            suspend_current_swap,
            cancel_and_refund,
            is_context_available,
            initialize_context,
            check_monero_node,
            check_electrum_node,
            get_wallet_descriptor,
            get_current_swap,
            get_data_dir,
            resolve_approval_request,
            redact,
            save_txt_files,
            get_monero_history,
            get_monero_main_address,
            get_monero_balance,
            send_monero,
            get_monero_sync_progress,
            check_seed,
            get_pending_approvals,
            set_monero_restore_height,
            reject_approval_request,
            get_restore_height
        ])
        .setup(setup)
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            RunEvent::Exit | RunEvent::ExitRequested { .. } => {
                // Here we cleanup the Context when the application is closed
                // This is necessary to among other things stop the monero-wallet-rpc process
                // If the application is forcibly closed, this may not be called.
                // TODO: fix that
                let state = app.state::<State>();
                let context_to_cleanup = if let Ok(context_lock) = state.context.try_read() {
                    context_lock.clone()
                } else {
                    println!("Failed to acquire lock on context");
                    None
                };

                if let Some(context) = context_to_cleanup {
                    if let Err(err) = context.cleanup() {
                        println!("Cleanup failed {}", err);
                    }
                }
            }
            _ => {}
        })
}

// Here we define the Tauri commands that will be available to the frontend
// The commands are defined using the `tauri_command!` macro.
// Implementations are handled by the Request trait
tauri_command!(get_balance, BalanceArgs);
tauri_command!(buy_xmr, BuyXmrArgs);
tauri_command!(resume_swap, ResumeSwapArgs);
tauri_command!(withdraw_btc, WithdrawBtcArgs);
tauri_command!(monero_recovery, MoneroRecoveryArgs);
tauri_command!(get_logs, GetLogsArgs);
tauri_command!(list_sellers, ListSellersArgs);
tauri_command!(cancel_and_refund, CancelAndRefundArgs);
tauri_command!(redact, RedactArgs);
tauri_command!(send_monero, SendMoneroArgs);

// These commands require no arguments
tauri_command!(get_wallet_descriptor, ExportBitcoinWalletArgs, no_args);
tauri_command!(suspend_current_swap, SuspendCurrentSwapArgs, no_args);
tauri_command!(get_swap_info, GetSwapInfoArgs);
tauri_command!(get_swap_infos_all, GetSwapInfosAllArgs, no_args);
tauri_command!(get_history, GetHistoryArgs, no_args);
tauri_command!(get_monero_addresses, GetMoneroAddressesArgs, no_args);
tauri_command!(get_monero_history, GetMoneroHistoryArgs, no_args);
tauri_command!(get_current_swap, GetCurrentSwapArgs, no_args);
tauri_command!(set_monero_restore_height, SetRestoreHeightArgs);
tauri_command!(get_restore_height, GetRestoreHeightArgs, no_args);
tauri_command!(get_monero_main_address, GetMoneroMainAddressArgs, no_args);
tauri_command!(get_monero_balance, GetMoneroBalanceArgs, no_args);
tauri_command!(get_monero_sync_progress, GetMoneroSyncProgressArgs, no_args);

/// Here we define Tauri commands whose implementation is not delegated to the Request trait
#[tauri::command]
async fn is_context_available(state: tauri::State<'_, State>) -> Result<bool, String> {
    // TODO: Here we should return more information about status of the context (e.g. initializing, failed)
    Ok(state.try_get_context().is_ok())
}

#[tauri::command]
async fn check_monero_node(
    args: CheckMoneroNodeArgs,
    _: tauri::State<'_, State>,
) -> Result<CheckMoneroNodeResponse, String> {
    args.request().await.to_string_result()
}

#[tauri::command]
async fn check_electrum_node(
    args: CheckElectrumNodeArgs,
    _: tauri::State<'_, State>,
) -> Result<CheckElectrumNodeResponse, String> {
    args.request().await.to_string_result()
}

#[tauri::command]
async fn check_seed(
    args: CheckSeedArgs,
    _: tauri::State<'_, State>,
) -> Result<CheckSeedResponse, String> {
    args.request().await.to_string_result()
}

// Returns the data directory
// This is independent of the context to ensure the user can open the directory even if the context cannot
// be initialized (for troubleshooting purposes)
#[tauri::command]
async fn get_data_dir(args: GetDataDirArgs, _: tauri::State<'_, State>) -> Result<String, String> {
    Ok(data::data_dir_from(None, args.is_testnet)
        .to_string_result()?
        .to_string_lossy()
        .to_string())
}

#[tauri::command]
async fn save_txt_files(
    app: tauri::AppHandle,
    zip_file_name: String,
    content: HashMap<String, String>,
) -> Result<(), String> {
    // Step 1: Get the owned PathBuf from the dialog
    let path_buf_from_dialog: tauri_plugin_dialog::FilePath = app
        .dialog()
        .file()
        .set_file_name(format!("{}.zip", &zip_file_name).as_str())
        .add_filter(&zip_file_name, &["zip"])
        .blocking_save_file() // This returns Option<PathBuf>
        .ok_or_else(|| "Dialog cancelled or file path not selected".to_string())?; // Converts to Result<PathBuf, String> and unwraps to PathBuf

    // Step 2: Now get a &Path reference from the owned PathBuf.
    // The user's code structure implied an .as_path().ok_or_else(...) chain which was incorrect for &Path.
    // We'll directly use the PathBuf, or if &Path is strictly needed:
    let selected_file_path: &std::path::Path = path_buf_from_dialog
        .as_path()
        .ok_or_else(|| "Could not convert file path".to_string())?;

    let zip_file = std::fs::File::create(selected_file_path)
        .map_err(|e| format!("Failed to create file: {}", e))?;

    let mut zip = ZipWriter::new(zip_file);

    for (filename, file_content_str) in content.iter() {
        zip.start_file(
            format!("{}.txt", filename).as_str(),
            SimpleFileOptions::default(),
        ) // Pass &str to start_file
        .map_err(|e| format!("Failed to start file {}: {}", &filename, e))?; // Use &filename

        zip.write_all(file_content_str.as_bytes())
            .map_err(|e| format!("Failed to write to file {}: {}", &filename, e))?;
        // Use &filename
    }

    zip.finish()
        .map_err(|e| format!("Failed to finish zip: {}", e))?;

    Ok(())
}

#[tauri::command]
async fn resolve_approval_request(
    args: ResolveApprovalArgs,
    state: tauri::State<'_, State>,
) -> Result<(), String> {
    let request_id = args
        .request_id
        .parse()
        .map_err(|e| format!("Invalid request ID '{}': {}", args.request_id, e))?;

    state
        .handle
        .resolve_approval(request_id, args.accept)
        .await
        .to_string_result()?;

    Ok(())
}

#[tauri::command]
async fn reject_approval_request(
    args: RejectApprovalArgs,
    state: tauri::State<'_, State>,
) -> Result<RejectApprovalResponse, String> {
    let request_id = args
        .request_id
        .parse()
        .map_err(|e| format!("Invalid request ID '{}': {}", args.request_id, e))?;

    state
        .handle
        .reject_approval(request_id)
        .await
        .to_string_result()?;

    Ok(RejectApprovalResponse { success: true })
}

#[tauri::command]
async fn get_pending_approvals(
    state: tauri::State<'_, State>,
) -> Result<GetPendingApprovalsResponse, String> {
    let approvals = state
        .handle
        .get_pending_approvals()
        .await
        .to_string_result()?;

    Ok(GetPendingApprovalsResponse { approvals })
}

/// Tauri command to initialize the Context
#[tauri::command]
async fn initialize_context(
    settings: TauriSettings,
    testnet: bool,
    state: tauri::State<'_, State>,
) -> Result<(), String> {
    // Lock at the beginning - fail immediately if already locked
    let mut context_lock = state
        .context
        .try_write()
        .map_err(|_| "Context is already being initialized".to_string())?;

    // Fail if the context is already initialized
    if context_lock.is_some() {
        return Err("Context is already initialized".to_string());
    }

    // Get tauri handle from the state
    let tauri_handle = state.handle.clone();

    // Notify frontend that the context is being initialized
    tauri_handle.emit_context_init_progress_event(TauriContextStatusEvent::Initializing);

    let context_result = ContextBuilder::new(testnet)
        .with_bitcoin(Bitcoin {
            bitcoin_electrum_rpc_urls: settings.electrum_rpc_urls.clone(),
            bitcoin_target_block: None,
        })
        .with_monero(settings.monero_node_config)
        .with_json(false)
        .with_debug(true)
        .with_tor(settings.use_tor)
        .with_tauri(tauri_handle.clone())
        .build()
        .await;

    match context_result {
        Ok(context_instance) => {
            *context_lock = Some(Arc::new(context_instance));

            tracing::info!("Context initialized");

            // Emit event to frontend
            tauri_handle.emit_context_init_progress_event(TauriContextStatusEvent::Available);
            Ok(())
        }
        Err(e) => {
            tracing::error!(error = ?e, "Failed to initialize context");

            // Emit event to frontend
            tauri_handle.emit_context_init_progress_event(TauriContextStatusEvent::Failed);
            Err(e.to_string())
        }
    }
}
