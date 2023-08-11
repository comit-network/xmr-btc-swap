use anyhow::Result;
use std::fmt::Debug;
use std::fs::OpenOptions;
use std::io;
use std::io::Write;
use std::option::Option::Some;
use std::path::{Path, PathBuf};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::format::{DefaultFields, Format, Json, JsonFields};
use tracing_subscriber::fmt::time::UtcTime;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt, FmtSubscriber, Layer, Registry, registry};
use tracing_subscriber::fmt::{FormatEvent, MakeWriter};
use serde_json::{json, Value};
use tracing::subscriber::set_global_default;


pub struct MyMakeWriter {
    dir: PathBuf,
    // other members like stdout and stderr if needed
}

impl MyMakeWriter {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
            // initialize other members
        }
    }

    fn get_log_path(&self, swap_id: &str) -> PathBuf {
        self.dir.join(format!("swap-{}.log", swap_id))
    }

    fn get_file_writer(&self, swap_id: &str) -> io::Result<impl Write> {
        OpenOptions::new().append(true).create(true).open(self.get_log_path(swap_id))
    }
}

impl<'a> MakeWriter<'a> for MyMakeWriter {
    type Writer = Box<dyn Write + 'a>;

    fn make_writer(&'a self) -> Self::Writer {
        unreachable!();
    }

    fn make_writer_for(&'a self, meta: &tracing::Metadata<'_>) -> Self::Writer {
        // Print all attributes of the event
        println!("Event attributes: {:?}", meta);
        let swap_id = "dummy-swap-id";

        Box::new(self.get_file_writer(swap_id).expect("Failed to open log file"))
    }
}


pub fn init(debug: bool, json: bool, dir: impl AsRef<Path>) -> Result<()> {
    let level_filter = EnvFilter::try_new("swap=debug")?;
    let registry = Registry::default().with(level_filter);


    let file_logger = registry.with(
        fmt::layer()
            .with_ansi(false)
            .with_target(false)
            .json()
            .with_writer(MyMakeWriter::new(dir))
    );

    set_global_default(file_logger.with(info_terminal_printer()))?;

    //tracing::info!("Logging initialized to {}", dir.as_ref().display());
    Ok(())
}

pub struct StdErrPrinter<L> {
    inner: L,
    level: Level,
}

type StdErrLayer<S, T> = fmt::Layer<
    S,
    DefaultFields,
    Format<fmt::format::Full, T>,
    fn() -> io::Stderr,
>;

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
        S: Subscriber + for<'a> registry::LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        if self.level.ge(event.metadata().level()) {
            self.inner.on_event(event, ctx);
        }
    }
}