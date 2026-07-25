use std::{
    io::{self, Write},
    sync::{Arc, Mutex, OnceLock},
};

use tracing_subscriber::{fmt::MakeWriter, EnvFilter};

static AUDIT_LOG_SINK: OnceLock<Arc<Mutex<Vec<u8>>>> = OnceLock::new();

pub fn audit_log_sink() -> Arc<Mutex<Vec<u8>>> {
    AUDIT_LOG_SINK
        .get_or_init(|| {
            let sink = Arc::new(Mutex::new(Vec::new()));
            let subscriber = tracing_subscriber::fmt()
                .json()
                .with_current_span(false)
                .with_span_list(false)
                .with_file(false)
                .with_line_number(false)
                .with_writer(SharedWriter(sink.clone()))
                .with_env_filter(EnvFilter::new("proofplane::audit=info"))
                .finish();
            tracing::subscriber::set_global_default(subscriber)
                .expect("integration audit tracing subscriber installs");

            sink
        })
        .clone()
}

#[derive(Clone)]
struct SharedWriter(Arc<Mutex<Vec<u8>>>);

impl<'a> MakeWriter<'a> for SharedWriter {
    type Writer = SharedWriterGuard;

    fn make_writer(&'a self) -> Self::Writer {
        SharedWriterGuard(self.0.clone())
    }
}

struct SharedWriterGuard(Arc<Mutex<Vec<u8>>>);

impl Write for SharedWriterGuard {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("audit log sink poisoned"))?
            .write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("audit log sink poisoned"))?
            .flush()
    }
}
