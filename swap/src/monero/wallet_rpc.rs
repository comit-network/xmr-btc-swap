use ::monero::Network;
use anyhow::{bail, Context, Error, Result};
use big_bytes::BigByte;
use data_encoding::HEXLOWER;
use futures::{StreamExt, TryStreamExt};
use monero_rpc::wallet::{Client, MoneroWalletRpc as _};
use reqwest::header::CONTENT_LENGTH;
use reqwest::Url;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::fs::{remove_file, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio_util::codec::{BytesCodec, FramedRead};
use tokio_util::io::StreamReader;

// See: https://www.moneroworld.com/#nodes, https://monero.fail
// We don't need any testnet nodes because we don't support testnet at all
const MONERO_DAEMONS: [MoneroDaemon; 17] = [
    MoneroDaemon::new("xmr-node.cakewallet.com", 18081, Network::Mainnet),
    MoneroDaemon::new("nodex.monerujo.io", 18081, Network::Mainnet),
    MoneroDaemon::new("node.moneroworld.com", 18089, Network::Mainnet),
    MoneroDaemon::new("nodes.hashvault.pro", 18081, Network::Mainnet),
    MoneroDaemon::new("p2pmd.xmrvsbeast.com", 18081, Network::Mainnet),
    MoneroDaemon::new("node.monerodevs.org", 18089, Network::Mainnet),
    MoneroDaemon::new("xmr-node-usa-east.cakewallet.com", 18081, Network::Mainnet),
    MoneroDaemon::new("xmr-node-uk.cakewallet.com", 18081, Network::Mainnet),
    MoneroDaemon::new("node.community.rino.io", 18081, Network::Mainnet),
    MoneroDaemon::new("testingjohnross.com", 20031, Network::Mainnet),
    MoneroDaemon::new("xmr.litepay.ch", 18081, Network::Mainnet),
    MoneroDaemon::new("node.trocador.app", 18089, Network::Mainnet),
    MoneroDaemon::new("stagenet.xmr-tw.org", 38081, Network::Stagenet),
    MoneroDaemon::new("node.monerodevs.org", 38089, Network::Stagenet),
    MoneroDaemon::new("singapore.node.xmr.pm", 38081, Network::Stagenet),
    MoneroDaemon::new("xmr-lux.boldsuck.org", 38081, Network::Stagenet),
    MoneroDaemon::new("stagenet.community.rino.io", 38081, Network::Stagenet),
];

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
compile_error!("unsupported operating system");

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const DOWNLOAD_URL: &str = "https://downloads.getmonero.org/cli/monero-mac-x64-v0.18.3.1.tar.bz2";
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const DOWNLOAD_HASH: &str = "7f8bd9364ef16482b418aa802a65be0e4cc660c794bb5d77b2d17bc84427883a";

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const DOWNLOAD_URL: &str = "https://downloads.getmonero.org/cli/monero-mac-armv8-v0.18.3.1.tar.bz2";
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const DOWNLOAD_HASH: &str = "915288b023cb5811e626e10052adc6ac5323dd283c5a25b91059b0fb86a21fb6";

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const DOWNLOAD_URL: &str = "https://downloads.getmonero.org/cli/monero-linux-x64-v0.18.3.1.tar.bz2";
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const DOWNLOAD_HASH: &str = "23af572fdfe3459b9ab97e2e9aa7e3c11021c955d6064b801a27d7e8c21ae09d";

#[cfg(all(target_os = "linux", target_arch = "arm"))]
const DOWNLOAD_URL: &str =
    "https://downloads.getmonero.org/cli/monero-linux-armv7-v0.18.3.1.tar.bz2";
#[cfg(all(target_os = "linux", target_arch = "arm"))]
const DOWNLOAD_HASH: &str = "2ea2c8898cbab88f49423f4f6c15f2a94046cb4bbe827493dd061edc0fd5f1ca";

#[cfg(target_os = "windows")]
const DOWNLOAD_URL: &str = "https://downloads.getmonero.org/cli/monero-win-x64-v0.18.3.1.zip";
#[cfg(target_os = "windows")]
const DOWNLOAD_HASH: &str = "35dcc4bee4caad3442659d37837e0119e4649a77f2e3b5e80dd6d9b8fc4fb6ad";

#[cfg(any(target_os = "macos", target_os = "linux"))]
const PACKED_FILE: &str = "monero-wallet-rpc";

#[cfg(target_os = "windows")]
const PACKED_FILE: &str = "monero-wallet-rpc.exe";

const WALLET_RPC_VERSION: &str = "v0.18.3.1";

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("monero wallet rpc executable not found in downloaded archive")]
pub struct ExecutableNotFoundInArchive;

pub struct WalletRpcProcess {
    _child: Child,
    port: u16,
}

struct MoneroDaemon {
    address: &'static str,
    port: u16,
    network: Network,
}

impl MoneroDaemon {
    const fn new(address: &'static str, port: u16, network: Network) -> Self {
        Self {
            address,
            port,
            network,
        }
    }

    /// Checks if the Monero daemon is available by sending a request to its `get_info` endpoint.
    async fn is_available(&self, client: &reqwest::Client) -> Result<bool, Error> {
        let url = format!("http://{}:{}/get_info", self.address, self.port);
        let res = client
            .get(url)
            .send()
            .await
            .context("Failed to send request to get_info endpoint")?;

        let json: MoneroDaemonGetInfoResponse = res
            .json()
            .await
            .context("Failed to deserialize daemon get_info response")?;

        let is_status_ok = json.status == "OK";
        let is_synchronized = json.synchronized;
        let is_correct_network = match self.network {
            Network::Mainnet => json.mainnet,
            Network::Stagenet => json.stagenet,
            Network::Testnet => json.testnet,
        };

        Ok(is_status_ok && is_synchronized && is_correct_network)
    }
}

impl Display for MoneroDaemon {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.address, self.port)
    }
}

#[derive(Deserialize)]
struct MoneroDaemonGetInfoResponse {
    status: String,
    synchronized: bool,
    mainnet: bool,
    stagenet: bool,
    testnet: bool,
}

/// Chooses an available Monero daemon based on the specified network.
async fn choose_monero_daemon(network: Network) -> Result<&'static MoneroDaemon, Error> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .https_only(false)
        .build()?;

    // We only want to check for daemons that match the specified network
    let network_matching_daemons = MONERO_DAEMONS
        .iter()
        .filter(|daemon| daemon.network == network);

    for daemon in network_matching_daemons {
        match daemon.is_available(&client).await {
            Ok(true) => {
                tracing::debug!(%daemon, "Found available Monero daemon");
                return Ok(daemon);
            }
            Err(err) => {
                tracing::debug!(%err, %daemon, "Failed to connect to Monero daemon");
                continue;
            }
            Ok(false) => continue,
        }
    }

    bail!("No Monero daemon could be found. Please specify one manually or try again later.")
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

        // check the monero-wallet-rpc version
        let exec_path = monero_wallet_rpc.exec_path();
        tracing::debug!("RPC exec path: {}", exec_path.display());

        if exec_path.exists() {
            let output = Command::new(&exec_path).arg("--version").output().await?;
            let version = String::from_utf8_lossy(&output.stdout);
            tracing::debug!("RPC version output: {}", version);

            if !version.contains(WALLET_RPC_VERSION) {
                tracing::info!("Removing old version of monero-wallet-rpc");
                tokio::fs::remove_file(exec_path).await?;
            }
        }

        // if monero-wallet-rpc doesn't exist then download it
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
                progress="0%",
                size=%content_length.big_byte(2),
                download_url=DOWNLOAD_URL,
                "Downloading monero-wallet-rpc",
            );

            let mut hasher = Sha256::new();

            let byte_stream = response
                .bytes_stream()
                .map_ok(|bytes| {
                    hasher.update(&bytes);
                    bytes
                })
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

            let (mut received, mut notified) = (0, 0);
            while let Some(chunk) = stream.next().await {
                let bytes = chunk?;
                received += bytes.len();
                // the stream is decompressed as it is downloaded
                // file is compressed approx 3:1 in bz format
                let total = 3 * content_length;
                let percent = 100 * received as u64 / total;
                if percent != notified && percent % 10 == 0 {
                    tracing::info!(
                        progress=format!("{}%", percent),
                        size=%content_length.big_byte(2),
                        download_url=DOWNLOAD_URL,
                        "Downloading monero-wallet-rpc",
                    );
                    notified = percent;
                }
                file.write_all(&bytes).await?;
            }

            tracing::info!(
                progress="100%",
                size=%content_length.big_byte(2),
                download_url=DOWNLOAD_URL,
                "Downloading monero-wallet-rpc",
            );

            let result = hasher.finalize();
            let result_hash = HEXLOWER.encode(result.as_ref());
            if result_hash != DOWNLOAD_HASH {
                bail!(
                    "SHA256 of download ({}) does not match expected ({})!",
                    result_hash,
                    DOWNLOAD_HASH
                );
            } else {
                tracing::debug!("Hashes match");
            }

            file.flush().await?;

            tracing::debug!("Extracting archive");
            Self::extract_archive(&monero_wallet_rpc).await?;
        }
        Ok(monero_wallet_rpc)
    }

    pub async fn run(
        &self,
        network: Network,
        daemon_address: Option<String>,
    ) -> Result<WalletRpcProcess> {
        let port = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await?
            .local_addr()?
            .port();

        let daemon_address = match daemon_address {
            Some(daemon_address) => daemon_address,
            None => choose_monero_daemon(network).await?.to_string(),
        };

        tracing::debug!(
            %daemon_address,
            %port,
            "Starting monero-wallet-rpc"
        );

        let network_flag = match network {
            Network::Mainnet => {
                vec![]
            }
            Network::Stagenet => {
                vec!["--stagenet"]
            }
            Network::Testnet => {
                vec!["--testnet"]
            }
        };

        let mut child = Command::new(self.exec_path())
            .env("LANG", "en_AU.UTF-8")
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .args(network_flag)
            .arg("--daemon-address")
            .arg(daemon_address)
            .arg("--rpc-bind-port")
            .arg(format!("{}", port))
            .arg("--disable-rpc-login")
            .arg("--wallet-dir")
            .arg(self.working_dir.join("monero-data"))
            .arg("--no-initial-sync")
            .spawn()?;

        let stdout = child
            .stdout
            .take()
            .expect("monero wallet rpc stdout was not piped parent process");

        let mut reader = BufReader::new(stdout).lines();

        #[cfg(not(target_os = "windows"))]
        while let Some(line) = reader.next_line().await? {
            if line.contains("Starting wallet RPC server") {
                break;
            }
        }

        // If we do not hear from the monero_wallet_rpc process for 3 seconds we assume
        // it is is ready
        #[cfg(target_os = "windows")]
        while let Ok(line) =
            tokio::time::timeout(std::time::Duration::from_secs(3), reader.next_line()).await
        {
            line?;
        }

        // Send a json rpc request to make sure monero_wallet_rpc is ready
        Client::localhost(port)?.get_version().await?;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn extract_host_and_port(address: String) -> (&'static str, u16) {
        let parts: Vec<&str> = address.split(':').collect();

        if parts.len() == 2 {
            let host = parts[0].to_string();
            let port = parts[1].parse::<u16>().unwrap();
            let static_str_host: &'static str = Box::leak(host.into_boxed_str());
            return (static_str_host, port);
        }
        panic!("Could not extract host and port from address: {}", address)
    }

    #[tokio::test]
    async fn test_is_daemon_available_success() {
        let mut server = mockito::Server::new();

        let _ = server
            .mock("GET", "/get_info")
            .with_status(200)
            .with_body(
                r#"
                {
                    "status": "OK",
                    "synchronized": true,
                    "mainnet": true,
                    "stagenet": false,
                    "testnet": false
                }
                "#,
            )
            .create();

        let (host, port) = extract_host_and_port(server.host_with_port());

        let client = reqwest::Client::new();
        let result = MoneroDaemon::new(host, port, Network::Mainnet)
            .is_available(&client)
            .await;

        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn test_is_daemon_available_wrong_network_failure() {
        let mut server = mockito::Server::new();

        let _ = server
            .mock("GET", "/get_info")
            .with_status(200)
            .with_body(
                r#"
                {
                    "status": "OK",
                    "synchronized": true,
                    "mainnet": true,
                    "stagenet": false,
                    "testnet": false
                }
                "#,
            )
            .create();

        let (host, port) = extract_host_and_port(server.host_with_port());

        let client = reqwest::Client::new();
        let result = MoneroDaemon::new(host, port, Network::Stagenet)
            .is_available(&client)
            .await;

        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn test_is_daemon_available_not_synced_failure() {
        let mut server = mockito::Server::new();

        let _ = server
            .mock("GET", "/get_info")
            .with_status(200)
            .with_body(
                r#"
                {
                    "status": "OK",
                    "synchronized": false,
                    "mainnet": true,
                    "stagenet": false,
                    "testnet": false
                }
                "#,
            )
            .create();

        let (host, port) = extract_host_and_port(server.host_with_port());

        let client = reqwest::Client::new();
        let result = MoneroDaemon::new(host, port, Network::Mainnet)
            .is_available(&client)
            .await;

        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn test_is_daemon_available_network_error_failure() {
        let client = reqwest::Client::new();
        let result = MoneroDaemon::new("does.not.exist.com", 18081, Network::Mainnet)
            .is_available(&client)
            .await;

        assert!(result.is_err());
    }
}
