use anyhow::Result;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::FmtSubscriber;

pub fn init(level: LevelFilter) -> Result<()> {
    if level == LevelFilter::OFF {
        return Ok(());
    }

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
