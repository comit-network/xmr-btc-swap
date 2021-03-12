use anyhow::{anyhow, Context, Result};
use druid::widget::{Button, Container, Flex, Label, LensWrap, TextBox};
use druid::{
    AppDelegate, AppLauncher, Color, Command, Data, DelegateCtx, Env, ExtEventSink, Handled,
    LocalizedString, MenuDesc, Selector, Target, Widget, WidgetExt, WindowDesc,
};
use libp2p::core::Multiaddr;
use libp2p::PeerId;
use std::str::FromStr;
use std::sync::Arc;
use swap::cli::config::Config;
use swap::database::Database;
use swap::execution_params;
use swap::execution_params::{ExecutionParams, GetExecutionParams};
use swap::protocol::bob;
use swap::protocol::bob::swap::SwapEventDetails;
use swap::protocol::bob::{Builder, EventLoop};
use swap::seed::Seed;
use swap::ui::model::bitcoin;
use swap::ui::model::swap::State;
use swap::ui::model::swap_amounts::SwapAmounts;
use swap::ui::widget;
use tokio::runtime::Handle;
use uuid::Uuid;

// TODO: Set to domain
// pub const DEFAULT_ALICE_MULTIADDR: &str =
// "/dns4/xmr-btc-asb.coblox.tech/tcp/9876";
pub const DEFAULT_ALICE_MULTIADDR: &str = "/ip4/192.168.1.7/tcp/9876";
pub const DEFAULT_ALICE_PEER_ID: &str = "12D3KooWCdMKjesXMJz1SiZ7HgotrxuqhQJbP5sgBm2BwP1cqThi";

const WINDOW_TITLE: LocalizedString<AppState> = LocalizedString::new("XMR<>BTC Swap");

const SWAP_AMOUNTS_UPDATE_EVENT: Selector<SwapAmounts> = Selector::new("swap-amounts-update-event");
const SWAP_EXEC_STATE_UPDATE_EVENT: Selector<State> = Selector::new("swap-exec-state-update-event");

const SWAP_START_BUTTON_CLICKED_EVENT: Selector = Selector::new("swap-start-triggered");

const TEXT_SIZE_S: f64 = 16.0f64;
const TEXT_SIZE_M: f64 = 18.0f64;

const MONERO_NETWORK: ::monero::network::Network = ::monero::network::Network::Stagenet;

#[derive(Clone, druid::Data, druid::Lens)]
struct AppState {
    counter: u32,

    // wallet
    bitcoin_balance: bitcoin::Amount,
    dai_balance: f64,
    ether_balance: f64,

    xmr_receive_address: String,
    xmr_receive_address_validation: String,

    swap_state: State,
    swap_amounts: SwapAmounts,
    swap_exec_state: State,
}

#[tokio::main]
async fn main() -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let handle = runtime.handle().clone();

    let config = Config::testnet();

    // hardcode to testnet/stagenet
    let bitcoin_network = swap::bitcoin::Network::Testnet;
    let monero_network = monero::Network::Stagenet;
    let execution_params = execution_params::Testnet::get_execution_params();

    // TODO: Duplicated because we need seed for wallet and network...
    let seed =
        Seed::from_file_or_generate(&config.data.dir).context("Failed to read in seed file")?;

    let bitcoin_wallet = init_bitcoin_wallet(bitcoin_network, &config, seed).await?;
    let (monero_wallet, _container) = init_monero_wallet(
        monero_network,
        &config,
        "monero-stagenet.exan.tech".to_string(),
        execution_params,
    )
    .await?;

    let main_window = WindowDesc::new(ui_builder)
        .title(WINDOW_TITLE)
        .menu(make_menu())
        .window_size((700.0, 600.0));
    let launcher = AppLauncher::with_window(main_window);
    let event_sink = launcher.get_external_handle();

    let bitcoin_wallet = Arc::new(bitcoin_wallet);
    let monero_wallet = Arc::new(monero_wallet);

    launcher
        .delegate(CommandHandler {
            event_sink,
            handle,
            bitcoin_wallet,
            monero_wallet,
            config,
            execution_params,
            alice_peer_id: PeerId::from_str(DEFAULT_ALICE_PEER_ID)
                .expect("constant to be parsable"),
            alice_address: Multiaddr::from_str(DEFAULT_ALICE_MULTIADDR)
                .expect("constant to be parsable"),
        })
        // TODO: sled logs are spamming...
        .use_simple_logger()
        .launch(AppState {
            counter: 0,
            bitcoin_balance: bitcoin::Amount::zero(),
            dai_balance: 2345.1234,
            ether_balance: 1.123456,
            swap_amounts: SwapAmounts {
                bitcoin: ::swap::bitcoin::Amount::ZERO,
                monero: ::swap::monero::Amount::ZERO,
            },
            swap_state: State::not_triggered(),
            swap_exec_state: State::none(),
            xmr_receive_address: "".to_string(),
            xmr_receive_address_validation: "".to_string(),
        })
        .map_err(|e| anyhow!(e))
}

fn ui_builder() -> impl Widget<AppState> {
    let start_swap_button = Button::new("start swap")
        .on_click(|ctx, _, _| {
            ctx.submit_command(Command::new(
                SWAP_START_BUTTON_CLICKED_EVENT,
                (),
                Target::Auto,
            ));
        })
        .padding(5.0);

    let xmr_receive_address = TextBox::new()
        .with_placeholder("xmr receive address")
        .with_text_size(TEXT_SIZE_M)
        .fix_width(400.0)
        .lens(AppState::xmr_receive_address);

    // TODO: Fix clippy warning - using to_string makes problems
    #[allow(clippy::useless_format)]
    let xmr_receive_address_label = Label::dynamic(|data, _| format!("{}", data))
        .with_text_size(TEXT_SIZE_S)
        .fix_width(400.0)
        .lens(AppState::xmr_receive_address_validation);

    #[allow(clippy::useless_format)]
    let swap_exec_state = Label::dynamic(|data, _| format!("{}", data))
        .with_text_size(TEXT_SIZE_S)
        .fix_width(400.0)
        .lens(AppState::swap_exec_state);

    let swap = Container::new(
        Flex::column()
            .must_fill_main_axis(true)
            .with_child(Label::new("Swap"))
            .with_spacer(20.)
            .with_child(xmr_receive_address)
            .with_child(Flex::row().with_child(xmr_receive_address_label))
            .with_child(Flex::row().with_child(start_swap_button))
            .with_child(Flex::row().with_child(swap_exec_state))
            .with_child(
                Flex::row().with_child(LensWrap::new(widget::swap_state(), AppState::swap_state)),
            )
            .with_child(Flex::row().with_child(LensWrap::new(
                widget::swap_amounts(),
                AppState::swap_amounts,
            )))
            .padding(20.)
            .expand_width(),
    )
    .border(Color::WHITE, 1.);

    Flex::column().with_child(swap).padding(20.)
}

#[derive(Clone)]
struct CommandHandler {
    event_sink: ExtEventSink,
    handle: Handle,

    bitcoin_wallet: Arc<swap::bitcoin::wallet::Wallet>,
    monero_wallet: Arc<swap::monero::wallet::Wallet>,
    config: Config,
    execution_params: ExecutionParams,

    alice_peer_id: PeerId,
    alice_address: Multiaddr,
}

impl AppDelegate<AppState> for CommandHandler {
    fn command(
        &mut self,
        _ctx: &mut DelegateCtx<'_>,
        _target: Target,
        cmd: &Command,
        data: &mut AppState,
        _env: &Env,
    ) -> Handled {
        if cmd.get(SWAP_START_BUTTON_CLICKED_EVENT).is_some() {
            // TODO: Ensure that the button looks disabled as well...
            if data.swap_state != State::not_triggered() {
                data.xmr_receive_address_validation =
                    "ℹ️️ Cannot trigger another swap until the current one is finished!".to_string();
                return Handled::Yes;
            }

            if data.xmr_receive_address.is_empty() {
                data.xmr_receive_address_validation =
                    "⚠️ Fill in XMR address before starting swap!".to_string();
                return Handled::No;
            }

            let xmr_address = match ::monero::Address::from_str(data.xmr_receive_address.as_str()) {
                Ok(xmr_address) => {
                    if xmr_address.network != MONERO_NETWORK {
                        data.xmr_receive_address_validation = format!(
                            "⚠️ The given XMR address is on network {:?} only {:?} accepted!",
                            xmr_address.network, MONERO_NETWORK
                        );
                        return Handled::No;
                    }
                    xmr_address
                }
                Err(_) => {
                    data.xmr_receive_address_validation =
                        "⚠️ The given XMR address is invalid!".to_string();
                    return Handled::No;
                }
            };

            // TODO: This might not affect anything...
            data.swap_state = State::triggered();
            data.xmr_receive_address_validation = "".to_string();
            print!(
                "xmr address to be passed to swap: {}",
                data.xmr_receive_address
            );

            let (swap_event_sender, mut swap_event_receiver) = tokio::sync::mpsc::channel(100);

            // TODO: Fix the unwraps in async block - does not allow ?
            let swap = {
                let cloned_self = self.clone();

                async move {
                    // TODO: Integrate wallet and funding into the UI - at the moment the
                    // application just 0.001 (and there is no balance check yet...)
                    // TODO: This should be handled by adding the periodic balance checker again. Or
                    // logic that triggers waiting for the swap to be funded...
                    let btc_swap_amount = swap::bitcoin::Amount::from_btc(0.001).unwrap();

                    let seed = Seed::from_file_or_generate(&cloned_self.config.data.dir)
                        .context("Failed to read in seed file")
                        .unwrap();

                    let (event_loop, event_loop_handle) = EventLoop::new(
                        &seed.derive_libp2p_identity(),
                        cloned_self.alice_peer_id,
                        cloned_self.alice_address.clone(),
                        cloned_self.bitcoin_wallet.clone(),
                    )
                    .unwrap();
                    let handle = cloned_self.handle.spawn(event_loop.run());

                    let db = Database::open(cloned_self.config.data.dir.join("database").as_path())
                        .context("Failed to open database")
                        .unwrap();

                    let swap = Builder::new(
                        db,
                        Uuid::new_v4(),
                        cloned_self.bitcoin_wallet.clone(),
                        cloned_self.monero_wallet.clone(),
                        cloned_self.execution_params,
                        event_loop_handle,
                        xmr_address,
                    )
                    .with_init_params(btc_swap_amount)
                    .build()
                    .unwrap();

                    let swap = bob::run(swap, Some(swap_event_sender));

                    // TODO: Reactivate that one can press swap button once swap finishes...
                    tokio::select! {
                        event_loop_result = handle => {
                            event_loop_result.unwrap().unwrap();
                        },
                        swap_result = swap => {
                            swap_result.unwrap();
                        }
                    }
                }
            };
            self.handle.spawn(swap);

            let swap_events = {
                let event_sink = self.event_sink.clone();
                async move {
                    while let Some(swap_event) = swap_event_receiver.recv().await {
                        match swap_event.details {
                            SwapEventDetails::State { state } => {
                                if event_sink
                                    .submit_command(
                                        SWAP_EXEC_STATE_UPDATE_EVENT,
                                        State::from(state),
                                        Target::Auto,
                                    )
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            SwapEventDetails::Amounts { bitcoin, monero } => {
                                if event_sink
                                    .submit_command(
                                        SWAP_AMOUNTS_UPDATE_EVENT,
                                        SwapAmounts { bitcoin, monero },
                                        Target::Auto,
                                    )
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        }
                    }
                }
            };
            self.handle.spawn(swap_events);

            data.swap_state = State::running();

            return Handled::No;
        };

        if let Some(swap_amounts) = cmd.get(SWAP_AMOUNTS_UPDATE_EVENT) {
            data.swap_amounts = *swap_amounts;
            return Handled::Yes;
        };

        if let Some(state) = cmd.get(SWAP_EXEC_STATE_UPDATE_EVENT) {
            data.swap_exec_state = state.clone();
            return Handled::Yes;
        };

        Handled::No
    }
}

#[allow(unused_assignments, unused_mut)]
fn make_menu<T: Data>() -> MenuDesc<T> {
    let mut base = MenuDesc::empty();
    #[cfg(target_os = "macos")]
    {
        base = base.append(druid::platform_menus::mac::application::default())
    }
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        base = base.append(druid::platform_menus::win::file::default());
    }
    base.append(
        MenuDesc::new(LocalizedString::new("common-menu-edit-menu"))
            .append(druid::platform_menus::common::undo())
            .append(druid::platform_menus::common::redo())
            .append_separator()
            .append(druid::platform_menus::common::cut().disabled())
            .append(druid::platform_menus::common::copy())
            .append(druid::platform_menus::common::paste()),
    )
}

async fn init_bitcoin_wallet(
    network: swap::bitcoin::Network,
    config: &Config,
    seed: Seed,
) -> Result<swap::bitcoin::Wallet> {
    let wallet_dir = config.data.dir.join("wallet");

    let wallet = swap::bitcoin::Wallet::new(
        config.bitcoin.electrum_rpc_url.clone(),
        config.bitcoin.electrum_http_url.clone(),
        network,
        &wallet_dir,
        seed.derive_extended_private_key(network)?,
    )
    .await
    .context("Failed to initialize Bitcoin wallet")?;

    wallet.sync().await?;

    Ok(wallet)
}

async fn init_monero_wallet(
    monero_network: swap::monero::Network,
    config: &Config,
    monero_daemon_host: String,
    execution_params: ExecutionParams,
) -> Result<(swap::monero::Wallet, swap::monero::WalletRpcProcess)> {
    const MONERO_BLOCKCHAIN_MONITORING_WALLET_NAME: &str = "swap-tool-blockchain-monitoring-wallet";

    let monero_wallet_rpc = swap::monero::WalletRpc::new(config.data.dir.join("monero")).await?;

    let monero_wallet_rpc_process = monero_wallet_rpc
        .run(monero_network, monero_daemon_host.as_str())
        .await?;

    let monero_wallet = swap::monero::Wallet::new(
        monero_wallet_rpc_process.endpoint(),
        monero_network,
        MONERO_BLOCKCHAIN_MONITORING_WALLET_NAME.to_string(),
        execution_params.monero_avg_block_time,
    );

    monero_wallet.open_or_create().await?;

    let _test_wallet_connection = monero_wallet
        .block_height()
        .await
        .context("Failed to validate connection to monero-wallet-rpc")?;

    Ok((monero_wallet, monero_wallet_rpc_process))
}
