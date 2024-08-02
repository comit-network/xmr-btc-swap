use std::path::Path;

use anyhow::Result;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::time::UtcTime;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::FmtSubscriber;

pub fn init(
    level: LevelFilter,
    json_format: bool,
    timestamp: bool,
    dir: impl AsRef<Path>,
) -> Result<()> {
    if level == LevelFilter::OFF {
        return Ok(());
    }

    let is_terminal = atty::is(atty::Stream::Stderr);

    let appender = tracing_appender::rolling::never(dir.as_ref(), "swap-all.log");

    let builder = FmtSubscriber::builder()
        .with_env_filter(format!("asb={},swap={}", level, level))
        .with_writer(appender.and(std::io::stderr))
        .with_ansi(is_terminal)
        .with_timer(UtcTime::rfc_3339())
        .with_target(false);

    match (json_format, timestamp) {
        (true, true) => builder.json().init(),
        (true, false) => builder.json().without_time().init(),
        (false, true) => builder.init(),
        (false, false) => builder.without_time().init(),
    }

    tracing::info!(%level, "Initialized tracing");

    Ok(())
}
