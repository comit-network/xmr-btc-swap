//! Eigensync server binary

use clap::{Parser, Subcommand};
use eigensync::{Server, ServerConfig};
use std::path::PathBuf;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(name = "eigensync-server")]
#[command(about = "Eigensync distributed state synchronization server")]
#[command(version)]
struct Cli {
    /// Configuration file path
    #[arg(short, long)]
    config: Option<PathBuf>,
    
    /// Database path
    #[arg(long)]
    database: Option<PathBuf>,
    
    /// Listen address
    #[arg(short, long, default_value = "0.0.0.0")]
    listen_address: String,
    
    /// Listen port
    #[arg(short, long, default_value = "9944")]
    port: u16,
    
    /// Maximum number of peers
    #[arg(long, default_value = "100")]
    max_peers: u32,
    
    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,
    
    /// Enable JSON logging
    #[arg(long)]
    json: bool,
    
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Clone, Subcommand)]
enum Commands {
    /// Run the server
    Run,
    /// Generate default configuration
    GenerateConfig {
        /// Output file path
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    
    // Initialize logging
    let filter = if cli.debug {
        "debug,eigensync=trace"
    } else {
        "info,eigensync=debug"
    };
    
    if cli.json {
        tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::new(filter))
            .with(tracing_subscriber::fmt::layer().json())
            .init();
    } else {
        tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::new(filter))
            .with(tracing_subscriber::fmt::layer().pretty())
            .init();
    }
    
    info!("Starting eigensync server v{}", env!("CARGO_PKG_VERSION"));
    
    // Handle subcommands
    let command = cli.command.clone().unwrap_or(Commands::Run);
    match command {
        Commands::Run => {
            run_server(cli).await?;
        }
        Commands::GenerateConfig { output } => {
            generate_config(output)?;
        }
    }
    
    Ok(())
}

async fn run_server(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    // Load or create configuration
    let mut config = if let Some(config_path) = cli.config {
        if config_path.exists() {
            info!("Loading configuration from {:?}", config_path);
            // TODO: Implement config file loading
            ServerConfig::default()
        } else {
            warn!("Configuration file {:?} not found, using defaults", config_path);
            ServerConfig::default()
        }
    } else {
        info!("No configuration file specified, using defaults");
        ServerConfig::default()
    };
    
    // Override config with CLI arguments
    if let Some(database_path) = cli.database {
        config.database_path = database_path;
    }
    config.listen_address = cli.listen_address;
    config.listen_port = cli.port;
    config.max_peers = cli.max_peers;
    
    info!("Server configuration:");
    info!("  Database: {:?}", config.database_path);
    info!("  Listen: {}:{}", config.listen_address, config.listen_port);
    info!("  Max peers: {}", config.max_peers);
    
    // Create and run server
    let server = Server::new(config).await?;
    
    // Set up signal handling for graceful shutdown
    let shutdown_signal = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install CTRL+C signal handler");
        info!("Received shutdown signal");
    };
    
    // Run server with graceful shutdown
    tokio::select! {
        result = server.run() => {
            match result {
                Ok(_) => info!("Server completed successfully"),
                Err(e) => {
                    tracing::error!("Server error: {}", e);
                    return Err(e.into());
                }
            }
        }
        _ = shutdown_signal => {
            info!("Shutting down server gracefully");
        }
    }
    
    Ok(())
}

fn generate_config(output: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let config = ServerConfig::default();
    let config_toml = toml::to_string_pretty(&config)?;
    
    match output {
        Some(path) => {
            std::fs::write(&path, config_toml)?;
            info!("Generated configuration file: {:?}", path);
        }
        None => {
            println!("{}", config_toml);
        }
    }
    
    Ok(())
} 