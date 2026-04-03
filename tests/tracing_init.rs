use std::io;
use std::sync::{Arc, Mutex};

use operator_console::tracing_setup::make_tracing_subscriber;
use tracing::{debug, info};
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
struct SharedBuffer {
    inner: Arc<Mutex<Vec<u8>>>,
}

struct SharedBufferWriter {
    inner: Arc<Mutex<Vec<u8>>>,
}

impl<'a> MakeWriter<'a> for SharedBuffer {
    type Writer = SharedBufferWriter;

    fn make_writer(&'a self) -> Self::Writer {
        SharedBufferWriter {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl io::Write for SharedBufferWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner
            .lock()
            .expect("buffer lock")
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn tracing_subscriber_uses_default_info_filter_when_rust_log_is_unset() {
    let _guard = EnvVarGuard::remove("RUST_LOG");
    let buffer = SharedBuffer::default();
    let subscriber = make_tracing_subscriber(buffer.clone());

    tracing::subscriber::with_default(subscriber, || {
        info!(target: "operator_console::tests", "info-visible");
        debug!(target: "operator_console::tests", "debug-hidden");
    });

    let output = String::from_utf8(buffer.inner.lock().expect("buffer lock").clone())
        .expect("utf8 log output");
    assert!(output.contains("info-visible"), "{output}");
    assert!(!output.contains("debug-hidden"), "{output}");
}

struct EnvVarGuard {
    name: &'static str,
    original: Option<String>,
}

impl EnvVarGuard {
    fn remove(name: &'static str) -> Self {
        let original = std::env::var(name).ok();
        unsafe {
            std::env::remove_var(name);
        }
        Self { name, original }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(value) => unsafe {
                std::env::set_var(self.name, value);
            },
            None => unsafe {
                std::env::remove_var(self.name);
            },
        }
    }
}
