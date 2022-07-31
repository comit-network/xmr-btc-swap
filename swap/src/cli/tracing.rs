use anyhow::Result;
use std::option::Option::Some;
use std::path::Path;
use time::format_description::well_known::Rfc3339;
use tracing::subscriber::set_global_default;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::format::{DefaultFields, Format, JsonFields};
use tracing_subscriber::fmt::time::UtcTime;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::{fmt, EnvFilter, FmtSubscriber, Layer, Registry};
use uuid::Uuid;

pub fn init(debug: bool, json: bool, dir: impl AsRef<Path>, swap_id: Option<Uuid>) -> Result<()> {
    if let Some(swap_id) = swap_id {
        let level_filter = EnvFilter::try_new("swap=debug")?;

        let registry = Registry::default().with(level_filter);

        let appender =
            tracing_appender::rolling::never(dir.as_ref(), format!("swap-{}.log", swap_id));
        let (appender, guard) = tracing_appender::non_blocking(appender);

        std::mem::forget(guard);

        let file_logger = registry.with(
            fmt::layer()
                .with_ansi(false)
                .with_target(false)
                .json()
                .with_writer(appender),
        );

        if json && debug {
            set_global_default(file_logger.with(debug_json_terminal_printer()))?;
        } else if json && !debug {
            set_global_default(file_logger.with(info_json_terminal_printer()))?;
        } else if !json && debug {
            set_global_default(file_logger.with(debug_terminal_printer()))?;
        } else {
            set_global_default(file_logger.with(info_terminal_printer()))?;
        }
    } else {
        let level = if debug { Level::DEBUG } else { Level::INFO };
        let is_terminal = atty::is(atty::Stream::Stderr);

        let builder = FmtSubscriber::builder()
            .with_env_filter(format!("swap={}", level))
            .with_writer(std::io::stderr)
            .with_ansi(is_terminal)
            .with_timer(UtcTime::rfc_3339())
            .with_target(false);

        if json {
            builder.json().init();
        } else {
            builder.init();
        }
    };

    tracing::info!("Logging initialized to {}", dir.as_ref().display());
    Ok(())
}

pub struct StdErrPrinter<L> {
    inner: L,
    level: Level,
}

type StdErrLayer<S, T> = tracing_subscriber::fmt::Layer<
    S,
    DefaultFields,
    Format<tracing_subscriber::fmt::format::Full, T>,
    fn() -> std::io::Stderr,
>;

type StdErrJsonLayer<S, T> = tracing_subscriber::fmt::Layer<
    S,
    JsonFields,
    Format<tracing_subscriber::fmt::format::Json, T>,
    fn() -> std::io::Stderr,
>;

fn debug_terminal_printer<S>() -> StdErrPrinter<StdErrLayer<S, UtcTime<Rfc3339>>> {
    let is_terminal = atty::is(atty::Stream::Stderr);
    StdErrPrinter {
        inner: fmt::layer()
            .with_ansi(is_terminal)
            .with_target(false)
            .with_timer(UtcTime::rfc_3339())
            .with_writer(std::io::stderr),
        level: Level::DEBUG,
    }
}

fn debug_json_terminal_printer<S>() -> StdErrPrinter<StdErrJsonLayer<S, UtcTime<Rfc3339>>> {
    let is_terminal = atty::is(atty::Stream::Stderr);
    StdErrPrinter {
        inner: fmt::layer()
            .with_ansi(is_terminal)
            .with_target(false)
            .with_timer(UtcTime::rfc_3339())
            .json()
            .with_writer(std::io::stderr),
        level: Level::DEBUG,
    }
}

fn info_terminal_printer<S>() -> StdErrPrinter<StdErrLayer<S, ()>> {
    let is_terminal = atty::is(atty::Stream::Stderr);
    StdErrPrinter {
        inner: fmt::layer()
            .with_ansi(is_terminal)
            .with_target(false)
            .with_level(false)
            .without_time()
            .with_writer(std::io::stderr),
        level: Level::INFO,
    }
}

fn info_json_terminal_printer<S>() -> StdErrPrinter<StdErrJsonLayer<S, ()>> {
    let is_terminal = atty::is(atty::Stream::Stderr);
    StdErrPrinter {
        inner: fmt::layer()
            .with_ansi(is_terminal)
            .with_target(false)
            .with_level(false)
            .without_time()
            .json()
            .with_writer(std::io::stderr),
        level: Level::INFO,
    }
}

impl<L, S> Layer<S> for StdErrPrinter<L>
where
    L: 'static + Layer<S>,
    S: Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        if self.level.ge(event.metadata().level()) {
            self.inner.on_event(event, ctx);
        }
    }
}
