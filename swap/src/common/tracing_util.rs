use std::io;
use std::path::Path;
use std::str::FromStr;

use anyhow::Result;
use tracing_subscriber::filter::{Directive, LevelFilter};
use tracing_subscriber::fmt::time::UtcTime;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter, Layer};

use crate::cli::api::tauri_bindings::{CliLogEmittedEvent, TauriEmitter, TauriHandle};

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
pub fn init(
    level_filter: LevelFilter,
    format: Format,
    dir: impl AsRef<Path>,
    tauri_handle: Option<TauriHandle>,
) -> Result<()> {
    // file logger will always write in JSON format and with timestamps
    let file_appender = tracing_appender::rolling::never(&dir, "swap-all.log");

    let file_layer = fmt::layer()
        .with_writer(file_appender)
        .with_ansi(false)
        .with_timer(UtcTime::rfc_3339())
        .with_target(false)
        .json()
        .with_filter(env_filter(level_filter)?);

    // terminal loger
    let is_terminal = atty::is(atty::Stream::Stderr);
    let terminal_layer = fmt::layer()
        .with_writer(std::io::stdout)
        .with_ansi(is_terminal)
        .with_timer(UtcTime::rfc_3339())
        .with_target(false);

    // tauri layer (forwards logs to the tauri guest when connected)
    let tauri_layer = fmt::layer()
        .with_writer(TauriWriter::new(tauri_handle))
        .with_ansi(false)
        .with_timer(UtcTime::rfc_3339())
        .with_target(false)
        .json()
        .with_filter(env_filter(level_filter)?);

    // combine the layers and start logging, format with json if specified
    if let Format::Json = format {
        tracing_subscriber::registry()
            .with(file_layer)
            .with(tauri_layer)
            .with(terminal_layer.json().with_filter(level_filter))
            .init();
    } else {
        tracing_subscriber::registry()
            .with(file_layer)
            .with(tauri_layer)
            .with(terminal_layer.with_filter(level_filter))
            .init();
    }

    // Now we can use the tracing macros to log messages
    tracing::info!(%level_filter, logs_dir=%dir.as_ref().display(), "Initialized tracing");

    Ok(())
}

/// This function controls which crate's logs actually get logged and from which level.
fn env_filter(level_filter: LevelFilter) -> Result<EnvFilter> {
    Ok(EnvFilter::from_default_env()
        .add_directive(Directive::from_str(&format!("asb={}", &level_filter))?)
        .add_directive(Directive::from_str(&format!("swap={}", &level_filter))?))
}

/// A writer that forwards tracing log messages to the tauri guest.
#[derive(Clone)]
pub struct TauriWriter {
    tauri_handle: Option<TauriHandle>,
}

impl TauriWriter {
    /// Create a new Tauri writer that sends log messages to the tauri guest.
    pub fn new(tauri_handle: Option<TauriHandle>) -> Self {
        Self { tauri_handle }
    }
}

/// This is needed for tracing to accept this as a writer.
impl<'a> MakeWriter<'a> for TauriWriter {
    type Writer = TauriWriter;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// For every write issued by tracing we simply pass the string on as an event to the tauri guest.
impl std::io::Write for TauriWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // Since this function accepts bytes, we need to pass to utf8 first
        let owned_buf = buf.to_owned();
        let utf8_string = String::from_utf8(owned_buf)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;

        // Then send to tauri
        self.tauri_handle.emit_cli_log_event(CliLogEmittedEvent {
            buffer: utf8_string,
        });

        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        // No-op, we don't need to flush anything
        Ok(())
    }
}
