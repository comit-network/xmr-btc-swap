use ::monero::Network;
use anyhow::{Context, Result};
use async_compression::tokio::bufread::BzDecoder;
use big_bytes::BigByte;
use futures::{StreamExt, TryStreamExt};
use reqwest::{header::CONTENT_LENGTH, Url};
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{
    fs::{remove_file, OpenOptions},
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
};
use tokio_tar::Archive;
use tokio_util::{
    codec::{BytesCodec, FramedRead},
    io::StreamReader,
};

#[cfg(target_os = "macos")]
const DOWNLOAD_URL: &str = "http://downloads.getmonero.org/cli/monero-mac-x64-v0.17.1.9.tar.bz2";

#[cfg(target_os = "linux")]
const DOWNLOAD_URL: &str = "https://downloads.getmonero.org/cli/monero-linux-x64-v0.17.1.9.tar.bz2";

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("unsupported operating system");

const PACKED_FILE: &str = "monero-wallet-rpc";

pub struct WalletRpcProcess {
    _child: Child,
    port: u16,
}

impl WalletRpcProcess {
    pub fn endpoint(&self) -> Url {
        Url::parse(&format!("http://127.0.0.1:{}/json_rpc", self.port))
            .expect("Static url template is always valid")
    }
}

pub struct WalletRpc {
    working_dir: PathBuf,
}

impl WalletRpc {
    pub async fn new(working_dir: impl AsRef<Path>) -> Result<WalletRpc> {
        let working_dir = working_dir.as_ref();

        if !working_dir.exists() {
            tokio::fs::create_dir(working_dir).await?;
        }

        let monero_wallet_rpc = WalletRpc {
            working_dir: working_dir.to_path_buf(),
        };

        if monero_wallet_rpc.tar_path().exists() {
            remove_file(monero_wallet_rpc.tar_path()).await?;
        }

        if !monero_wallet_rpc.exec_path().exists() {
            let mut options = OpenOptions::new();
            let mut file = options
                .read(true)
                .write(true)
                .create_new(true)
                .open(monero_wallet_rpc.tar_path())
                .await?;

            let response = reqwest::get(DOWNLOAD_URL).await?;

            let content_length = response.headers()[CONTENT_LENGTH]
                .to_str()
                .context("failed to convert content-length to string")?
                .parse::<u64>()?;

            tracing::info!(
                "Downloading monero-wallet-rpc ({})",
                content_length.big_byte(2)
            );

            let byte_stream = response
                .bytes_stream()
                .map_err(|err| std::io::Error::new(ErrorKind::Other, err));

            let mut stream = FramedRead::new(
                BzDecoder::new(StreamReader::new(byte_stream)),
                BytesCodec::new(),
            )
            .map_ok(|bytes| bytes.freeze());

            while let Some(chunk) = stream.next().await {
                file.write(&chunk?).await?;
            }

            file.flush().await?;

            let mut options = OpenOptions::new();
            let file = options
                .read(true)
                .open(monero_wallet_rpc.tar_path())
                .await?;

            let mut ar = Archive::new(file);
            let mut entries = ar.entries()?;

            while let Some(file) = entries.next().await {
                let mut f = file?;
                if f.path()?
                    .to_str()
                    .context("Could not find convert path to str in tar ball")?
                    .contains(PACKED_FILE)
                {
                    f.unpack(monero_wallet_rpc.exec_path()).await?;
                }
            }

            remove_file(monero_wallet_rpc.tar_path()).await?;
        }

        Ok(monero_wallet_rpc)
    }
    pub async fn run(&self, network: Network, daemon_host: &str) -> Result<WalletRpcProcess> {
        let port = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await?
            .local_addr()?
            .port();

        tracing::debug!("Starting monero-wallet-rpc on port {}", port);

        let mut child = Command::new(self.exec_path())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .arg(match network {
                Network::Mainnet => "--mainnet",
                Network::Stagenet => "--stagenet",
                Network::Testnet => "--testnet",
            })
            .arg("--daemon-host")
            .arg(daemon_host)
            .arg("--rpc-bind-port")
            .arg(format!("{}", port))
            .arg("--disable-rpc-login")
            .arg("--wallet-dir")
            .arg(self.working_dir.join("monero-data"))
            .spawn()?;

        let stdout = child
            .stdout
            .take()
            .expect("monero wallet rpc stdout was not piped parent process");

        let mut reader = BufReader::new(stdout).lines();

        while let Some(line) = reader.next_line().await? {
            if line.contains("Starting wallet RPC server") {
                break;
            }
        }

        Ok(WalletRpcProcess {
            _child: child,
            port,
        })
    }

    fn tar_path(&self) -> PathBuf {
        self.working_dir.join("monero-cli-wallet.tar")
    }

    fn exec_path(&self) -> PathBuf {
        self.working_dir.join(PACKED_FILE)
    }
}
