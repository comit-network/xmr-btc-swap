use ::monero::Network;
use anyhow::{Context, Result};
use big_bytes::BigByte;
use futures::{StreamExt, TryStreamExt};
use reqwest::header::CONTENT_LENGTH;
use reqwest::Url;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::fs::{remove_file, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio_util::codec::{BytesCodec, FramedRead};
use tokio_util::io::StreamReader;

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
compile_error!("unsupported operating system");

#[cfg(target_os = "macos")]
const DOWNLOAD_URL: &str = "http://downloads.getmonero.org/cli/monero-mac-x64-v0.17.1.9.tar.bz2";

#[cfg(target_os = "linux")]
const DOWNLOAD_URL: &str = "https://downloads.getmonero.org/cli/monero-linux-x64-v0.17.1.9.tar.bz2";

#[cfg(target_os = "windows")]
const DOWNLOAD_URL: &str = "https://downloads.getmonero.org/cli/monero-win-x64-v0.17.1.9.zip";

#[cfg(any(target_os = "macos", target_os = "linux"))]
const PACKED_FILE: &str = "monero-wallet-rpc";

#[cfg(target_os = "windows")]
const PACKED_FILE: &str = "monero-wallet-rpc.exe";

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("monero wallet rpc executable not found in downloaded archive")]
pub struct ExecutableNotFoundInArchive;

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

        if monero_wallet_rpc.archive_path().exists() {
            remove_file(monero_wallet_rpc.archive_path()).await?;
        }

        if !monero_wallet_rpc.exec_path().exists() {
            let mut options = OpenOptions::new();
            let mut file = options
                .read(true)
                .write(true)
                .create_new(true)
                .open(monero_wallet_rpc.archive_path())
                .await?;

            let response = reqwest::get(DOWNLOAD_URL).await?;

            let content_length = response.headers()[CONTENT_LENGTH]
                .to_str()
                .context("Failed to convert content-length to string")?
                .parse::<u64>()?;

            tracing::info!(
                "Downloading monero-wallet-rpc ({})",
                content_length.big_byte(2)
            );

            let byte_stream = response
                .bytes_stream()
                .map_err(|err| std::io::Error::new(ErrorKind::Other, err));

            #[cfg(not(target_os = "windows"))]
            let mut stream = FramedRead::new(
                async_compression::tokio::bufread::BzDecoder::new(StreamReader::new(byte_stream)),
                BytesCodec::new(),
            )
            .map_ok(|bytes| bytes.freeze());

            #[cfg(target_os = "windows")]
            let mut stream = FramedRead::new(StreamReader::new(byte_stream), BytesCodec::new())
                .map_ok(|bytes| bytes.freeze());

            while let Some(chunk) = stream.next().await {
                file.write(&chunk?).await?;
            }

            file.flush().await?;

            Self::extract_archive(&monero_wallet_rpc).await?;
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

    fn archive_path(&self) -> PathBuf {
        self.working_dir.join("monero-cli-wallet.archive")
    }

    fn exec_path(&self) -> PathBuf {
        self.working_dir.join(PACKED_FILE)
    }

    #[cfg(not(target_os = "windows"))]
    async fn extract_archive(monero_wallet_rpc: &Self) -> Result<()> {
        use anyhow::bail;
        use tokio_tar::Archive;

        let mut options = OpenOptions::new();
        let file = options
            .read(true)
            .open(monero_wallet_rpc.archive_path())
            .await?;

        let mut ar = Archive::new(file);
        let mut entries = ar.entries()?;

        loop {
            match entries.next().await {
                Some(file) => {
                    let mut f = file?;
                    if f.path()?
                        .to_str()
                        .context("Could not find convert path to str in tar ball")?
                        .contains(PACKED_FILE)
                    {
                        f.unpack(monero_wallet_rpc.exec_path()).await?;
                        break;
                    }
                }
                None => bail!(ExecutableNotFoundInArchive),
            }
        }

        remove_file(monero_wallet_rpc.archive_path()).await?;

        Ok(())
    }

    #[cfg(target_os = "windows")]
    async fn extract_archive(monero_wallet_rpc: &Self) -> Result<()> {
        use std::fs::File;
        use tokio::task::JoinHandle;
        use zip::ZipArchive;

        let archive_path = monero_wallet_rpc.archive_path();
        let exec_path = monero_wallet_rpc.exec_path();

        let extract: JoinHandle<Result<()>> = tokio::task::spawn_blocking(|| {
            let file = File::open(archive_path)?;
            let mut zip = ZipArchive::new(file)?;

            let name = zip
                .file_names()
                .find(|name| name.contains(PACKED_FILE))
                .context(ExecutableNotFoundInArchive)?
                .to_string();

            let mut rpc = zip.by_name(&name)?;
            let mut file = File::create(exec_path)?;
            std::io::copy(&mut rpc, &mut file)?;
            Ok(())
        });
        extract.await??;

        remove_file(monero_wallet_rpc.archive_path()).await?;

        Ok(())
    }
}
