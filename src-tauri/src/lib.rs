use std::result::Result;
use std::sync::Arc;
use swap::cli::{
    api::{
        request::{
            BalanceArgs, BuyXmrArgs, GetHistoryArgs, GetSwapInfosAllArgs, MoneroRecoveryArgs,
            ResumeSwapArgs, SuspendCurrentSwapArgs, WithdrawBtcArgs,
        },
        tauri_bindings::{TauriContextStatusEvent, TauriEmitter, TauriHandle},
        Context, ContextBuilder,
    },
    command::{Bitcoin, Monero},
};
use tauri::{async_runtime::RwLock, Manager, RunEvent};

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
///
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
            context: tauri::State<'_, RwLock<State>>,
            args: $request_name,
        ) -> Result<<$request_name as swap::cli::api::request::Request>::Response, String> {
            // Throw error if context is not available
            let context = context.read().await.try_get_context()?;

            <$request_name as swap::cli::api::request::Request>::request(args, context)
                .await
                .to_string_result()
        }
    };
    ($fn_name:ident, $request_name:ident, no_args) => {
        #[tauri::command]
        async fn $fn_name(
            context: tauri::State<'_, RwLock<State>>,
        ) -> Result<<$request_name as swap::cli::api::request::Request>::Response, String> {
            // Throw error if context is not available
            let context = context.read().await.try_get_context()?;

            <$request_name as swap::cli::api::request::Request>::request($request_name {}, context)
                .await
                .to_string_result()
        }
    };
}

/// Represents the shared Tauri state. It is accessed by Tauri commands
struct State {
    pub context: Option<Arc<Context>>,
}

impl State {
    /// Creates a new State instance with no Context
    fn new() -> Self {
        Self { context: None }
    }

    /// Sets the context for the application state
    /// This is typically called after the Context has been initialized
    /// in the setup function
    fn set_context(&mut self, context: impl Into<Option<Arc<Context>>>) {
        self.context = context.into();
    }

    /// Attempts to retrieve the context
    /// Returns an error if the context is not available
    fn try_get_context(&self) -> Result<Arc<Context>, String> {
        self.context
            .clone()
            .ok_or("Context not available")
            .to_string_result()
    }
}

/// Sets up the Tauri application
/// Initializes the Tauri state and spawns an async task to set up the Context
fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let app_handle = app.app_handle().to_owned();

    // We need to set a value for the Tauri state right at the start
    // If we don't do this, Tauri commands will panic at runtime if no value is present
    app_handle.manage::<RwLock<State>>(RwLock::new(State::new()));

    tauri::async_runtime::spawn(async move {
        let tauri_handle = TauriHandle::new(app_handle.clone());

        let context = ContextBuilder::new(true)
            .with_bitcoin(Bitcoin {
                bitcoin_electrum_rpc_url: None,
                bitcoin_target_block: None,
            })
            .with_monero(Monero {
                monero_daemon_address: None,
            })
            .with_json(false)
            .with_debug(true)
            .with_tauri(tauri_handle.clone())
            .build()
            .await;

        match context {
            Ok(context) => {
                let state = app_handle.state::<RwLock<State>>();

                state.write().await.set_context(Arc::new(context));

                // To display to the user that the setup is done, we emit an event to the Tauri frontend
                tauri_handle.emit_context_init_progress_event(TauriContextStatusEvent::Available);
            }
            Err(e) => {
                println!("Error while initializing context: {:?}", e);

                // To display to the user that the setup failed, we emit an event to the Tauri frontend
                tauri_handle.emit_context_init_progress_event(TauriContextStatusEvent::Failed);
            }
        }
    });

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            get_balance,
            get_swap_infos_all,
            withdraw_btc,
            buy_xmr,
            resume_swap,
            get_history,
            monero_recovery,
            suspend_current_swap,
            is_context_available,
        ])
        .setup(setup)
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            RunEvent::Exit | RunEvent::ExitRequested { .. } => {
                // Here we cleanup the Context when the application is closed
                // This is necessary to among other things stop the monero-wallet-rpc process
                // If the application is forcibly closed, this may not be called
                let context = app.state::<RwLock<State>>().inner().try_read();

                match context {
                    Ok(context) => {
                        if let Some(context) = context.context.as_ref() {
                            if let Err(err) = context.cleanup() {
                                println!("Cleanup failed {}", err);
                            }
                        }
                    }
                    Err(err) => {
                        println!("Failed to acquire lock on context: {}", err);
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

// These commands require no arguments
tauri_command!(suspend_current_swap, SuspendCurrentSwapArgs, no_args);
tauri_command!(get_swap_infos_all, GetSwapInfosAllArgs, no_args);
tauri_command!(get_history, GetHistoryArgs, no_args);

/// Here we define Tauri commands whose implementation is not delegated to the Request trait
#[tauri::command]
async fn is_context_available(context: tauri::State<'_, RwLock<State>>) -> Result<bool, String> {
    Ok(context.read().await.try_get_context().is_ok())
}