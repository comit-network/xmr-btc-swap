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

    fn on_event(&self, event: &tracing::Event<'_>, ctx: tracing_subscriber::layer::Context<'_, S>) {
        if let Some(current_span_id) = ctx.current_span().id() {
            if let Some(span) = ctx.span(current_span_id) {
                if let Some(swap_id) = span.extensions().get::<String>() {
                    println!("swap_id: {}", swap_id);

                    // Here I need to figure out how to format the event in JSON just like the internal JSON formatter does
                    self.append_to_file(swap_id, &format!("{}\n", event.metadata().fields())).expect("Failed to write to file");
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
