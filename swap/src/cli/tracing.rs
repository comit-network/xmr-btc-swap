use anyhow::Result;
use std::fmt::Debug;
use std::fs::OpenOptions;
use std::io::Write;
use std::option::Option::Some;
use std::path::{Path, PathBuf};
use tracing::field::Field;
use tracing::span::Attributes;
use tracing::{Id, Level, Subscriber};
use tracing_subscriber::fmt::format::{Format, Json, JsonFields};
use tracing_subscriber::fmt::time::UtcTime;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, FmtSubscriber, Layer, Registry};
use tracing_subscriber::fmt::{format, FormatEvent, MakeWriter};
use serde_json::{json, Value};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct LogEvent<'a> {
    message: String,
    level: &'a str,
    target: &'a str,
    module: &'a str,
    file: Option<&'a str>,
    line: Option<u32>,
    fields: Value,
}

/// Transforms a `tracing::Event` into a JSON-formatted string.
fn format_event_as_json<'a, S>(
    _: &Context<'a, S>,
    event: &'a tracing::Event<'a>,
) -> serde_json::Result<String> {
    // Extracting fields from the event into a serde_json::Value
    let mut fields = json!({});
    let mut message = String::new();  // For capturing the main message

    event.record(&mut |field: &Field, value: &dyn Debug| {
        if field.name() == "message" {
            message = format!("{:?}", value);
        } else {
            fields[field.name()] = json!(format!("{:?}", value));
        }
    });

    let log = LogEvent {
        message, // Use the captured message here
        level: event.metadata().level().as_str(),
        target: event.metadata().target(),
        module: event.metadata().module_path().unwrap_or_default(),
        file: event.metadata().file(),
        line: event.metadata().line(),
        fields,
    };

    serde_json::to_string(&log)
}

struct SwapIdVisitor {
    swap_id: Option<String>,
}

pub struct FileLayer {
    dir: PathBuf,
}

impl FileLayer {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
        }
    }

    fn get_log_path(&self, swap_id: &str) -> PathBuf {
        self.dir.join(format!("swap-{}.log", swap_id))
    }

    fn append_to_file(&self, swap_id: &str, message: &str) -> std::io::Result<()> {
        let path = self.get_log_path(swap_id);
        let mut file = OpenOptions::new().append(true).create(true).open(path)?;
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
                    // TODO: This is a hack, I need to figure out how to get the JSON formatter to work

                    if let Ok(json_log) = format_event_as_json(&ctx, event) {
                        self.append_to_file(swap_id, &format!("{}\n", json_log))
                            .expect("Failed to write to file");
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
    let level = if debug { Level::DEBUG } else { Level::INFO };
    let is_terminal = atty::is(atty::Stream::Stderr);

    let file_layer = FileLayer::new(dir.as_ref());

    FmtSubscriber::builder()
        .with_env_filter(format!("swap={}", level))
        .with_writer(std::io::stderr)
        .with_ansi(is_terminal)
        .with_timer(UtcTime::rfc_3339())
        .with_target(false)
        .finish()
        .with(file_layer)
        .init();

    tracing::info!("Logging initialized to {}", dir.as_ref().display());
    Ok(())
}
