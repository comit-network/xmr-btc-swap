use anyhow::Result;
use std::path::Path;
use tracing::subscriber::set_global_default;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::format::{DefaultFields, Format};
use tracing_subscriber::fmt::time::ChronoLocal;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::{fmt, EnvFilter, FmtSubscriber, Layer, Registry};
use uuid::Uuid;

pub fn init(debug: bool, json: bool, dir: impl AsRef<Path>, swap_id: Uuid) -> Result<()> {
    if json {
        let level = if debug { Level::DEBUG } else { Level::INFO };

        let is_terminal = atty::is(atty::Stream::Stderr);

        FmtSubscriber::builder()
            .with_env_filter(format!("swap={}", level))
            .with_writer(std::io::stderr)
            .with_ansi(is_terminal)
            .with_timer(ChronoLocal::with_format("%F %T".to_owned()))
            .with_target(false)
            .json()
            .init();

        Ok(())
    } else {
        let level_filter = EnvFilter::try_new("swap=debug")?;

        let registry = Registry::default().with(level_filter);

        let appender = tracing_appender::rolling::never(dir, format!("swap-{}.log", swap_id));
        let (appender, guard) = tracing_appender::non_blocking(appender);

        std::mem::forget(guard);

        let file_logger = fmt::layer()
            .with_ansi(false)
            .with_target(false)
            .with_writer(appender);

        if debug {
            set_global_default(registry.with(file_logger).with(debug_terminal_printer()))?;
        } else {
            set_global_default(registry.with(file_logger).with(info_terminal_printer()))?;
        }

        Ok(())
    }
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

fn debug_terminal_printer<S>() -> StdErrPrinter<StdErrLayer<S, ChronoLocal>> {
    let is_terminal = atty::is(atty::Stream::Stderr);
    StdErrPrinter {
        inner: fmt::layer()
            .with_ansi(is_terminal)
            .with_target(false)
            .with_timer(ChronoLocal::with_format("%F %T".to_owned()))
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
