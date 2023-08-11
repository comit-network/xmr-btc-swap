use std::collections::HashMap;
use anyhow::Result;
use std::fmt::Debug;
use std::fs::OpenOptions;
use std::io::Write;
use std::option::Option::Some;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tracing::field::Field;
use tracing::span::Attributes;
use tracing::{Event, Id, Level, Subscriber};
use tracing_subscriber::fmt::format::{DefaultFields, Format, Json, JsonFields};
use tracing_subscriber::fmt::time::UtcTime;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::{EnvFilter, fmt, Layer, Registry};
use serde_json::{json, Value};
use serde::Serialize;
use time::format_description::well_known::Rfc3339;
use tracing::subscriber::set_global_default;

#[derive(Debug, Serialize)]
struct LogEvent<'a> {
    level: &'a str,
    fields: Value,
}

/// Transforms a `tracing::Event` into a JSON-formatted string.
fn format_event_as_json<'a, S>(
    _: &Context<'a, S>,
    event: &'a tracing::Event<'a>,
) -> serde_json::Result<String> {
    // Extracting fields from the event into a serde_json::Value
    let mut fields = json!({});

    event.record(&mut |field: &Field, value: &dyn Debug| {
        fields[field.name()] = json!(format!("{:?}", value));
    });

    let log = LogEvent {
        level: event.metadata().level().as_str(),
        fields,
    };

    serde_json::to_string(&log)
}

struct SwapIdVisitor {
    swap_id: Option<String>,
}

pub struct FileLayer {
    dir: PathBuf,
    file_handles: Mutex<HashMap<String, std::fs::File>>,
}

impl FileLayer {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
            file_handles: Mutex::new(HashMap::new()),
        }
    }

    fn get_log_path(&self, swap_id: String) -> PathBuf {
        self.dir.join(format!("swap-{}.log", swap_id))
    }

    fn append_to_file(&self, swap_id: String, message: &str) -> std::io::Result<()> {
        let mut cache = self.file_handles.lock().unwrap();
        let swap_id_clone = swap_id.clone();
        let file = cache.entry(swap_id).or_insert_with(|| {
            println!("Opening file for swap log {}", swap_id_clone);
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.get_log_path(swap_id_clone))
                .expect("Failed to open file")
        });
        file.write_all(message.as_bytes())
    }
}


impl<S> Layer<S> for FileLayer
    where
        S: Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut visitor = SwapIdVisitor { swap_id: None };
        attrs.record(&mut visitor);

        if let Some(swap_id) = visitor.swap_id {
            if let Some(span) = ctx.span(id) {
                span.extensions_mut().insert(swap_id);
            }
        }
    }

    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        if let Some(current_span_id) = ctx.current_span().id() {
            if let Some(span) = ctx.span(current_span_id) {
                if let Some(swap_id) = span.extensions().get::<String>() {
                    if let Ok(json_log) = format_event_as_json(&ctx, event) {
                        if let Err(err) = self.append_to_file(swap_id.clone(), &format!("{}\n", json_log)) {
                            println!("Failed to write log to assigned swap log file: {}", err);
                        }
                    }
                }
            }
        }
    }
}

impl tracing::field::Visit for SwapIdVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        if field.name() == "swap_id" {
            self.swap_id = Some(format!("{:?}", value));
        }
    }
}

pub fn init(debug: bool, json: bool, dir: impl AsRef<Path>) -> Result<()> {
    let level_filter = EnvFilter::try_new("swap=debug")?;
    let file_layer = FileLayer::new(dir.as_ref());

    let registry = Registry::default().with(level_filter).with(file_layer);

    if json && debug {
        set_global_default(registry.with(debug_json_terminal_printer()))?;
    } else if json && !debug {
        set_global_default(registry.with(info_json_terminal_printer()))?;
    } else if !json && debug {
        set_global_default(registry.with(debug_terminal_printer()))?;
    } else {
        set_global_default(registry.with(info_terminal_printer()))?;
    }

    tracing::info!("Logging initialized to {}", dir.as_ref().display());
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
    fn() -> std::io::Stderr,
>;

type StdErrJsonLayer<S, T> = fmt::Layer<
    S,
    JsonFields,
    Format<Json, T>,
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