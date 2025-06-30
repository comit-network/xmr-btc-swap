use clap::Parser;
use monero_rpc_pool::{config::Config, run_server};
use tracing::info;
use tracing_subscriber::{self, EnvFilter};

use monero::Network;

fn parse_network(s: &str) -> Result<Network, String> {
    match s.to_lowercase().as_str() {
        "mainnet" => Ok(Network::Mainnet),
        "stagenet" => Ok(Network::Stagenet),
        "testnet" => Ok(Network::Testnet),
        _ => Err(format!(
            "Invalid network: {}. Must be mainnet, stagenet, or testnet",
            s
        )),
    }
}

// TODO: Replace with Display impl for Network
fn network_to_string(network: &Network) -> String {
    match network {
        Network::Mainnet => "mainnet".to_string(),
        Network::Stagenet => "stagenet".to_string(),
        Network::Testnet => "testnet".to_string(),
    }
}

#[derive(Parser)]
#[command(name = "monero-rpc-pool")]
#[command(about = "A load-balancing HTTP proxy for Monero RPC nodes")]
#[command(version)]
struct Args {
    #[arg(long, default_value = "127.0.0.1")]
    #[arg(help = "Host address to bind the server to")]
    host: String,

    #[arg(short, long, default_value = "18081")]
    #[arg(help = "Port to bind the server to")]
    port: u16,

    #[arg(short, long, default_value = "mainnet")]
    #[arg(help = "Network to use for automatic node discovery")]
    #[arg(value_parser = parse_network)]
    network: Network,

    #[arg(short, long)]
    #[arg(help = "Enable verbose logging")]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("trace"))
        .with_target(false)
        .with_file(true)
        .with_line_number(true)
        .init();

    let config = Config::new_with_port(
        args.host,
        args.port,
        std::env::temp_dir().join("monero-rpc-pool"),
    );

    info!(
        host = config.host,
        port = config.port,
        network = network_to_string(&args.network),
        "Starting Monero RPC Pool"
    );

    if let Err(e) = run_server(config, args.network).await {
        eprintln!("Server error: {}", e);
        std::process::exit(1);
    }

    Ok(())
}
