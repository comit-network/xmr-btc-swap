use clap::Parser;
use tracing::{info, warn};
use tracing_subscriber::{self, EnvFilter};

use monero_rpc_pool::database::Database;
use monero_rpc_pool::discovery::NodeDiscovery;
use monero_rpc_pool::{config::Config, run_server};

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

    #[arg(long, value_delimiter = ',')]
    #[arg(help = "Comma-separated list of Monero node URLs (overrides network-based discovery)")]
    nodes: Option<Vec<String>>,

    #[arg(short, long, default_value = "mainnet")]
    #[arg(help = "Network to use for automatic node discovery")]
    #[arg(value_parser = parse_network)]
    network: Network,

    #[arg(short, long)]
    #[arg(help = "Enable verbose logging")]
    verbose: bool,
}

// Custom filter function that overrides log levels for our crate
fn create_level_override_filter(base_filter: &str) -> EnvFilter {
    // Parse the base filter and modify it to treat all monero_rpc_pool logs as trace
    let mut filter = EnvFilter::new(base_filter);

    // Add a directive that treats all levels from our crate as trace
    filter = filter.add_directive("monero_rpc_pool=trace".parse().unwrap());

    filter
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Create a filter that treats all logs from our crate as traces
    let base_filter = if args.verbose {
        // In verbose mode, show logs from other crates at WARN level
        "warn"
    } else {
        // In normal mode, show logs from other crates at ERROR level
        "error"
    };

    let filter = create_level_override_filter(base_filter);

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_file(true)
        .with_line_number(true)
        .init();

    // Store node count for later logging before potentially moving args.nodes
    let manual_node_count = args.nodes.as_ref().map(|nodes| nodes.len());

    // Determine nodes to use and set up discovery
    let _nodes = if let Some(manual_nodes) = args.nodes {
        info!(
            "Using manually specified nodes for network: {}",
            network_to_string(&args.network)
        );

        // Insert manual nodes into database with network information
        let db = Database::new().await?;
        let discovery = NodeDiscovery::new(db.clone())?;
        let mut parsed_nodes = Vec::new();

        for node_url in &manual_nodes {
            // Parse the URL to extract components
            if let Ok(url) = url::Url::parse(node_url) {
                let scheme = url.scheme().to_string();
                let _protocol = if scheme == "https" { "ssl" } else { "tcp" };
                let host = url.host_str().unwrap_or("").to_string();
                let port = url
                    .port()
                    .unwrap_or(if scheme == "https" { 443 } else { 80 })
                    as i64;

                let full_url = format!("{}://{}:{}", scheme, host, port);

                // Insert into database
                if let Err(e) = db
                    .upsert_node(&scheme, &host, port, &network_to_string(&args.network))
                    .await
                {
                    warn!("Failed to insert manual node {}: {}", node_url, e);
                } else {
                    parsed_nodes.push(full_url);
                }
            } else {
                warn!("Failed to parse manual node URL: {}", node_url);
            }
        }

        // Use manual nodes for discovery
        discovery
            .discover_and_insert_nodes(args.network, manual_nodes)
            .await?;
        parsed_nodes
    } else {
        info!(
            "Setting up automatic node discovery for {} network",
            network_to_string(&args.network)
        );
        let db = Database::new().await?;
        let discovery = NodeDiscovery::new(db.clone())?;

        // Start discovery process
        discovery.discover_nodes_from_sources(args.network).await?;
        Vec::new() // Return empty vec for consistency
    };

    let config = Config::new_with_port(
        args.host,
        args.port,
        std::env::temp_dir().join("monero-rpc-pool"),
    );

    let node_count_msg = if args.verbose {
        match manual_node_count {
            Some(count) => format!("{} manual nodes configured", count),
            None => "using automatic discovery".to_string(),
        }
    } else {
        "configured".to_string()
    };

    info!(
        "Starting Monero RPC Pool\nConfiguration:\n  Host: {}\n  Port: {}\n  Network: {}\n  Nodes: {}",
        config.host, config.port, network_to_string(&args.network), node_count_msg
    );

    if let Err(e) = run_server(config, args.network).await {
        eprintln!("Server error: {}", e);
        std::process::exit(1);
    }

    Ok(())
}
