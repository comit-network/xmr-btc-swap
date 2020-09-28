use std::{collections::HashMap, env::var, thread::sleep, time::Duration};
use testcontainers::{
    core::{Container, Docker, Port, WaitForMessage},
    Image,
};

pub const MONEROD_RPC_PORT: u16 = 48081;
pub const MINER_WALLET_RPC_PORT: u16 = 48083;
pub const ALICE_WALLET_RPC_PORT: u16 = 48084;
pub const BOB_WALLET_RPC_PORT: u16 = 48085;

#[derive(Debug)]
pub struct Monero {
    tag: String,
    args: Args,
    ports: Option<Vec<Port>>,
    entrypoint: Option<String>,
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
            .wait_for_message(
                "The daemon is running offline and will not attempt to sync to the Monero network",
            )
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

    fn ports(&self) -> Option<Vec<Port>> {
        self.ports.clone()
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
            ports: None,
            entrypoint: Some("".into()),
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

    pub fn with_mapped_port<P: Into<Port>>(mut self, port: P) -> Self {
        let mut ports = self.ports.unwrap_or_default();
        ports.push(port.into());
        self.ports = Some(ports);
        self
    }

    pub fn with_wallet(self, name: &str, rpc_port: u16) -> Self {
        let wallet = WalletArgs::new(name, rpc_port);
        let mut wallet_args = self.args.wallets;
        wallet_args.push(wallet);
        Self {
            args: Args {
                monerod: self.args.monerod,
                wallets: wallet_args,
            },
            ..self
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Args {
    monerod: MonerodArgs,
    wallets: Vec<WalletArgs>,
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

        args.join(" ")
    }
}

impl WalletArgs {
    pub fn new(wallet_dir: &str, rpc_port: u16) -> Self {
        let daemon_address = format!("localhost:{}", MONEROD_RPC_PORT);
        WalletArgs {
            disable_rpc_login: true,
            confirm_external_bind: true,
            wallet_dir: wallet_dir.into(),
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
        let mut args = Vec::new();

        args.push("/bin/bash".into());
        args.push("-c".into());

        let wallet_args: Vec<String> = self.wallets.iter().map(|wallet| wallet.args()).collect();
        let wallet_args = wallet_args.join(" & ");

        let cmd = format!("{} & {} ", self.monerod.args(), wallet_args);
        args.push(cmd);

        args.into_iter()
    }
}
