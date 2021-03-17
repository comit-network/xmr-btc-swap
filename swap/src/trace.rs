use anyhow::Result;
use tracing_log::LogTracer;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::FmtSubscriber;

pub fn init_tracing(level: LevelFilter) -> Result<()> {
    if level == LevelFilter::OFF {
        return Ok(());
    }

    // We want upstream library log messages, just only at Info level.
    LogTracer::init_with_filter(tracing_log::log::LevelFilter::Info)?;

    let is_terminal = atty::is(atty::Stream::Stderr);

    let builder = FmtSubscriber::builder()
        .with_env_filter(format!("asb={},swap={}", level, level))
        .with_writer(std::io::stderr)
        .with_ansi(is_terminal)
        .with_target(false);

    if !is_terminal {
        builder.without_time().init();
    } else {
        builder.init();
    }

    tracing::info!("Initialized tracing with level: {}", level);

    Ok(())
}
