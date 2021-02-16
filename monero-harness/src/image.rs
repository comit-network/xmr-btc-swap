use std::{collections::HashMap, env::var, thread::sleep, time::Duration};
use testcontainers::{
    core::{Container, Docker, WaitForMessage},
    Image,
};

pub const MONEROD_DAEMON_CONTAINER_NAME: &str = "monerod";
pub const MONEROD_DEFAULT_NETWORK: &str = "monero_network";
pub const MONEROD_RPC_PORT: u16 = 48081;
pub const WALLET_RPC_PORT: u16 = 48083;

#[derive(Debug)]
pub struct Monero {
    tag: String,
    args: Args,
    entrypoint: Option<String>,
    wait_for_message: String,
}

impl Image for Monero {
    type Args = Args;
    type EnvVars = HashMap<String, String>;
    type Volumes = HashMap<String, String>;
    type EntryPoint = str;

    fn descriptor(&self) -> String {
        format!("xmrto/monero:{}", self.tag)
    }

    fn wait_until_ready<D: Docker>(&self, container: &Container<'_, D, Self>) {
        container
            .logs()
            .stdout
            .wait_for_message(&self.wait_for_message)
            .unwrap();

        let additional_sleep_period =
            var("MONERO_ADDITIONAL_SLEEP_PERIOD").map(|value| value.parse());

        if let Ok(Ok(sleep_period)) = additional_sleep_period {
            let sleep_period = Duration::from_millis(sleep_period);

            sleep(sleep_period)
        }
    }

    fn args(&self) -> <Self as Image>::Args {
        self.args.clone()
    }

    fn volumes(&self) -> Self::Volumes {
        HashMap::new()
    }

    fn env_vars(&self) -> Self::EnvVars {
        HashMap::new()
    }

    fn with_args(self, args: <Self as Image>::Args) -> Self {
        Monero { args, ..self }
    }

    fn with_entrypoint(self, entrypoint: &Self::EntryPoint) -> Self {
        Self {
            entrypoint: Some(entrypoint.to_string()),
            ..self
        }
    }

    fn entrypoint(&self) -> Option<String> {
        self.entrypoint.to_owned()
    }
}

impl Default for Monero {
    fn default() -> Self {
        Monero {
            tag: "v0.16.0.3".into(),
            args: Args::default(),
            entrypoint: Some("".into()),
            wait_for_message: "core RPC server started ok".to_string(),
        }
    }
}

impl Monero {
    pub fn with_tag(self, tag_str: &str) -> Self {
        Monero {
            tag: tag_str.to_string(),
            ..self
        }
    }

    pub fn wallet(name: &str, daemon_address: String) -> Self {
        let wallet = WalletArgs::new(name, daemon_address, WALLET_RPC_PORT);
        let default = Monero::default();
        Self {
            args: Args {
                image_args: ImageArgs::WalletArgs(wallet),
            },
            wait_for_message: "Run server thread name: RPC".to_string(),
            ..default
        }
    }
}

#[derive(Clone, Debug)]
pub struct Args {
    image_args: ImageArgs,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            image_args: ImageArgs::MonerodArgs(MonerodArgs::default()),
        }
    }
}

#[derive(Clone, Debug)]
pub enum ImageArgs {
    MonerodArgs(MonerodArgs),
    WalletArgs(WalletArgs),
}

impl ImageArgs {
    fn args(&self) -> String {
        match self {
            ImageArgs::MonerodArgs(monerod_args) => monerod_args.args(),
            ImageArgs::WalletArgs(wallet_args) => wallet_args.args(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MonerodArgs {
    pub regtest: bool,
    pub offline: bool,
    pub rpc_payment_allow_free_loopback: bool,
    pub confirm_external_bind: bool,
    pub non_interactive: bool,
    pub no_igd: bool,
    pub hide_my_port: bool,
    pub rpc_bind_ip: String,
    pub rpc_bind_port: u16,
    pub fixed_difficulty: u32,
    pub data_dir: String,
    pub log_level: u32,
}

#[derive(Debug, Clone)]
pub struct WalletArgs {
    pub disable_rpc_login: bool,
    pub confirm_external_bind: bool,
    pub wallet_dir: String,
    pub rpc_bind_ip: String,
    pub rpc_bind_port: u16,
    pub daemon_address: String,
    pub log_level: u32,
}

/// Sane defaults for a mainnet regtest instance.
impl Default for MonerodArgs {
    fn default() -> Self {
        MonerodArgs {
            regtest: true,
            offline: true,
            rpc_payment_allow_free_loopback: true,
            confirm_external_bind: true,
            non_interactive: true,
            no_igd: true,
            hide_my_port: true,
            rpc_bind_ip: "0.0.0.0".to_string(),
            rpc_bind_port: MONEROD_RPC_PORT,
            fixed_difficulty: 1,
            data_dir: "/monero".to_string(),
            log_level: 2,
        }
    }
}

impl MonerodArgs {
    // Return monerod args as is single string so we can pass it to bash.
    fn args(&self) -> String {
        let mut args = vec!["monerod".to_string()];

        if self.regtest {
            args.push("--regtest".to_string())
        }

        if self.offline {
            args.push("--offline".to_string())
        }

        if self.rpc_payment_allow_free_loopback {
            args.push("--rpc-payment-allow-free-loopback".to_string())
        }

        if self.confirm_external_bind {
            args.push("--confirm-external-bind".to_string())
        }

        if self.non_interactive {
            args.push("--non-interactive".to_string())
        }

        if self.no_igd {
            args.push("--no-igd".to_string())
        }

        if self.hide_my_port {
            args.push("--hide-my-port".to_string())
        }

        if !self.rpc_bind_ip.is_empty() {
            args.push(format!("--rpc-bind-ip {}", self.rpc_bind_ip));
        }

        if self.rpc_bind_port != 0 {
            args.push(format!("--rpc-bind-port {}", self.rpc_bind_port));
        }

        if !self.data_dir.is_empty() {
            args.push(format!("--data-dir {}", self.data_dir));
        }

        if self.fixed_difficulty != 0 {
            args.push(format!("--fixed-difficulty {}", self.fixed_difficulty));
        }

        if self.log_level != 0 {
            args.push(format!("--log-level {}", self.log_level));
        }

        args.join(" ")
    }
}

impl WalletArgs {
    pub fn new(wallet_name: &str, daemon_address: String, rpc_port: u16) -> Self {
        WalletArgs {
            disable_rpc_login: true,
            confirm_external_bind: true,
            wallet_dir: wallet_name.into(),
            rpc_bind_ip: "0.0.0.0".into(),
            rpc_bind_port: rpc_port,
            daemon_address,
            log_level: 4,
        }
    }

    // Return monero-wallet-rpc args as is single string so we can pass it to bash.
    fn args(&self) -> String {
        let mut args = vec!["monero-wallet-rpc".to_string()];

        if self.disable_rpc_login {
            args.push("--disable-rpc-login".to_string())
        }

        if self.confirm_external_bind {
            args.push("--confirm-external-bind".to_string())
        }

        if !self.wallet_dir.is_empty() {
            args.push(format!("--wallet-dir {}", self.wallet_dir));
        }

        if !self.rpc_bind_ip.is_empty() {
            args.push(format!("--rpc-bind-ip {}", self.rpc_bind_ip));
        }

        if self.rpc_bind_port != 0 {
            args.push(format!("--rpc-bind-port {}", self.rpc_bind_port));
        }

        if !self.daemon_address.is_empty() {
            args.push(format!("--daemon-address {}", self.daemon_address));
        }

        if self.log_level != 0 {
            args.push(format!("--log-level {}", self.log_level));
        }

        args.join(" ")
    }
}

impl IntoIterator for Args {
    type Item = String;
    type IntoIter = ::std::vec::IntoIter<String>;

    fn into_iter(self) -> <Self as IntoIterator>::IntoIter {
        vec![
            "/bin/bash".to_string(),
            "-c".to_string(),
            format!("{} ", self.image_args.args()),
        ]
        .into_iter()
    }
}
