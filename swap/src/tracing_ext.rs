#![allow(clippy::unwrap_used)] // This is only meant to be used in tests.

use std::io;
use std::sync::{Arc, Mutex};
use tracing::subscriber;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;

/// Setup tracing with a capturing writer, allowing assertions on the log
/// messages.
///
/// Time and ANSI are disabled to make the output more predictable and
/// readable.
pub fn capture_logs(min_level: LevelFilter) -> MakeCapturingWriter {
    let make_writer = MakeCapturingWriter::default();

    let guard = subscriber::set_default(
        tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_writer(make_writer.clone())
            .with_env_filter(format!("{}", min_level))
            .finish(),
    );
    // don't clean up guard we stay initialized
    std::mem::forget(guard);

    make_writer
}

#[derive(Default, Clone)]
pub struct MakeCapturingWriter {
    writer: CapturingWriter,
}

impl MakeCapturingWriter {
    pub fn captured(&self) -> String {
        let captured = &self.writer.captured;
        let cursor = captured.lock().unwrap();
        String::from_utf8(cursor.clone().into_inner()).unwrap()
    }
}

impl<'a> MakeWriter<'a> for MakeCapturingWriter {
    type Writer = CapturingWriter;

    fn make_writer(&self) -> Self::Writer {
        self.writer.clone()
    }
}

#[derive(Default, Clone)]
pub struct CapturingWriter {
    captured: Arc<Mutex<io::Cursor<Vec<u8>>>>,
}

impl io::Write for CapturingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.captured.lock().unwrap().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
