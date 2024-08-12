use std::path::Path;

use anyhow::Result;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::time::UtcTime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::fmt;



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

    // File logger will always write in JSON format and with timestamps
    let file_appender = tracing_appender::rolling::never(dir.as_ref(), "swap-all.log");

    let file_layer = fmt::layer()
    .with_writer(std::io::stdout)
    .with_ansi(false)
    .with_timer(UtcTime::rfc_3339())
    .with_target(false)
    .json();

    // Terminal logger
    let terminal_layer_base = fmt::layer() 
        .with_writer(std::io::stdout)
        .with_ansi(is_terminal)
        .with_timer(UtcTime::rfc_3339())
        .with_target(false);

    // Since tracing is stupid, and makes each option return a different type
    // but also doesn't allow dynamic dispatch we have to use this beauty
    let (
        a,
        b, 
        c, 
        d
    ) = match (json_format, timestamp) {
        (true, true) => (Some(terminal_layer_base.json()), None, None, None),
        (true, false) => (None, Some(terminal_layer_base.json().without_time()), None, None),
        (false, true) => (None, None, Some(terminal_layer_base), None),
        (false, false) => (None, None, None, Some(terminal_layer_base.without_time())),
    };

    let combined_subscriber = tracing_subscriber::registry()
        .with(file_layer)
        .with(a);

    combined_subscriber.init();
    
    // let builder = FmtSubscriber::builder()
    //     .with_env_filter(format!("asb={},swap={}", level, level))
    //     .with_writer(async_file_appender.and(std::io::stderr))
    //     .with_ansi(is_terminal)
    //     .with_timer(UtcTime::rfc_3339())
    //     .with_target(false);

    


    // match (json_format, timestamp) {
    //     (true, true) => builder.json().init(),
    //     (true, false) => builder.json().without_time().init(),
    //     (false, true) => builder.init(),
    //     (false, false) => builder.without_time().init(),
    // }

    tracing::info!(%level, "Initialized tracing");

    Ok(())
}
