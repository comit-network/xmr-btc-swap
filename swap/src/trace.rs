use atty::{self};
use log::LevelFilter;
use tracing::{info, subscriber};
use tracing_log::LogTracer;
use tracing_subscriber::FmtSubscriber;

pub fn init_tracing(level: LevelFilter) -> anyhow::Result<()> {
    if level == LevelFilter::Off {
        return Ok(());
    }

    // We want upstream library log messages, just only at Info level.
    LogTracer::init_with_filter(LevelFilter::Info)?;

    let is_terminal = atty::is(atty::Stream::Stderr);
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(format!(
            "swap={},monero_harness={},bitcoin_harness={},http=warn,warp=warn",
            level, level, level,
        ))
        .with_writer(std::io::stderr)
        .with_ansi(is_terminal)
        .finish();

    subscriber::set_global_default(subscriber)?;
    info!("Initialized tracing with level: {}", level);

    Ok(())
}
