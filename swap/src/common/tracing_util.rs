use std::path::Path;
use std::str::FromStr;

use anyhow::Result;
use tracing_subscriber::filter::{Directive, LevelFilter};
use tracing_subscriber::fmt::time::UtcTime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter, Layer};

/// Output formats for logging messages.
pub enum Format {
    /// Standard, human readable format.
    Raw,
    /// JSON, machine readable format.
    Json,
}

/// Initialize tracing and enable logging messages according to these options.
/// Besides printing to `stdout`, this will append to a log file.
/// Said file will contain JSON-formatted logs of all levels,
/// disregarding the arguments to this function.
pub fn init(level_filter: LevelFilter, format: Format, dir: impl AsRef<Path>) -> Result<()> {
    let env_filter = EnvFilter::from_default_env()
        .add_directive(Directive::from_str(&format!("asb={}", &level_filter))?)
        .add_directive(Directive::from_str(&format!("swap={}", &level_filter))?);

    // file logger will always write in JSON format and with timestamps
    let file_appender = tracing_appender::rolling::never(&dir, "swap-all.log");

    let file_layer = fmt::layer()
        .with_writer(file_appender)
        .with_ansi(false)
        .with_timer(UtcTime::rfc_3339())
        .with_target(false)
        .json()
        .with_filter(env_filter);

    // terminal logger
    let is_terminal = atty::is(atty::Stream::Stderr);
    let terminal_layer = fmt::layer()
        .with_writer(std::io::stdout)
        .with_ansi(is_terminal)
        .with_timer(UtcTime::rfc_3339())
        .with_target(false);

    // combine the layers and start logging, format with json if specified
    if let Format::Json = format {
        tracing_subscriber::registry()
            .with(file_layer)
            .with(terminal_layer.json().with_filter(level_filter))
            .init();
    } else {
        tracing_subscriber::registry()
            .with(file_layer)
            .with(terminal_layer.with_filter(level_filter))
            .init();
    }

    // now we can use the tracing macros to log messages
    tracing::info!(%level_filter, logs_dir=%dir.as_ref().display(), "Initialized tracing");

    Ok(())
}
