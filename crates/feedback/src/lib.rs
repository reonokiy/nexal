use std::collections::VecDeque;
use std::io::Write;
use std::io::{self};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Result;
use nexal_protocol::ThreadId;
use nexal_protocol::protocol::SessionSource;
use feedback_diagnostics::FeedbackDiagnostics;
use tracing::Level;
use tracing_subscriber::Layer;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::fmt::writer::MakeWriter;
use tracing_subscriber::registry::LookupSpan;

pub mod feedback_diagnostics;

const DEFAULT_MAX_BYTES: usize = 4 * 1024 * 1024; // 4 MiB

#[derive(Clone)]
pub struct NexalFeedback {
    inner: Arc<FeedbackInner>,
}

impl Default for NexalFeedback {
    fn default() -> Self {
        Self::new()
    }
}

impl NexalFeedback {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_MAX_BYTES)
    }

    pub(crate) fn with_capacity(max_bytes: usize) -> Self {
        Self {
            inner: Arc::new(FeedbackInner::new(max_bytes)),
        }
    }

    pub(crate) fn make_writer(&self) -> FeedbackMakeWriter {
        FeedbackMakeWriter {
            inner: self.inner.clone(),
        }
    }

    /// Returns a [`tracing_subscriber`] layer that captures full-fidelity logs into this feedback
    /// ring buffer.
    pub fn logger_layer<S>(&self) -> impl Layer<S> + Send + Sync + 'static
    where
        S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    {
        tracing_subscriber::fmt::layer()
            .with_writer(self.make_writer())
            .with_timer(tracing_subscriber::fmt::time::SystemTime)
            .with_ansi(false)
            .with_target(false)
            .with_filter(Targets::new().with_default(Level::TRACE))
    }

    /// Returns a [`tracing_subscriber`] layer that collects structured metadata for feedback.
    pub fn metadata_layer<S>(&self) -> impl Layer<S> + Send + Sync + 'static
    where
        S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    {
        // Return an identity/passthrough layer that does nothing.
        tracing_subscriber::layer::Identity::new()
            .with_filter(Targets::new().with_default(Level::TRACE))
    }

    pub fn snapshot(&self, session_id: Option<ThreadId>) -> FeedbackSnapshot {
        let bytes = {
            let guard = self.inner.ring.lock().expect("mutex poisoned");
            guard.snapshot_bytes()
        };
        FeedbackSnapshot {
            bytes,
            feedback_diagnostics: FeedbackDiagnostics::collect_from_env(),
            thread_id: session_id
                .map(|id| id.to_string())
                .unwrap_or("no-active-thread-".to_string() + &ThreadId::new().to_string()),
        }
    }
}

struct FeedbackInner {
    ring: Mutex<RingBuffer>,
}

impl FeedbackInner {
    fn new(max_bytes: usize) -> Self {
        Self {
            ring: Mutex::new(RingBuffer::new(max_bytes)),
        }
    }
}

#[derive(Clone)]
pub(crate) struct FeedbackMakeWriter {
    inner: Arc<FeedbackInner>,
}

impl<'a> MakeWriter<'a> for FeedbackMakeWriter {
    type Writer = FeedbackWriter;

    fn make_writer(&'a self) -> Self::Writer {
        FeedbackWriter {
            inner: self.inner.clone(),
        }
    }
}

pub(crate) struct FeedbackWriter {
    inner: Arc<FeedbackInner>,
}

impl Write for FeedbackWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut guard = self.inner.ring.lock().map_err(|_| io::ErrorKind::Other)?;
        guard.push_bytes(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct RingBuffer {
    max: usize,
    buf: VecDeque<u8>,
}

impl RingBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            max: capacity,
            buf: VecDeque::with_capacity(capacity),
        }
    }

    fn len(&self) -> usize {
        self.buf.len()
    }

    fn push_bytes(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }

        if data.len() >= self.max {
            self.buf.clear();
            let start = data.len() - self.max;
            self.buf.extend(data[start..].iter().copied());
            return;
        }

        let needed = self.len() + data.len();
        if needed > self.max {
            let to_drop = needed - self.max;
            for _ in 0..to_drop {
                let _ = self.buf.pop_front();
            }
        }

        self.buf.extend(data.iter().copied());
    }

    fn snapshot_bytes(&self) -> Vec<u8> {
        self.buf.iter().copied().collect()
    }
}

pub struct FeedbackSnapshot {
    #[cfg_attr(not(test), allow(dead_code))]
    bytes: Vec<u8>,
    feedback_diagnostics: FeedbackDiagnostics,
    pub thread_id: String,
}

impl FeedbackSnapshot {
    #[cfg(test)]
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn feedback_diagnostics(&self) -> &FeedbackDiagnostics {
        &self.feedback_diagnostics
    }

    pub fn with_feedback_diagnostics(mut self, feedback_diagnostics: FeedbackDiagnostics) -> Self {
        self.feedback_diagnostics = feedback_diagnostics;
        self
    }

    /// No-op: feedback upload is disabled.
    pub fn upload_feedback(
        &self,
        _classification: &str,
        _reason: Option<&str>,
        _include_logs: bool,
        _extra_attachment_paths: &[PathBuf],
        _session_source: Option<SessionSource>,
        _logs_override: Option<Vec<u8>>,
    ) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::fmt::writer::MakeWriter;

    #[test]
    fn ring_buffer_drops_front_when_full() {
        let fb = NexalFeedback::with_capacity(8);
        {
            let mut w = fb.make_writer().make_writer();
            w.write_all(b"abcdefgh").unwrap();
            w.write_all(b"ij").unwrap();
        }
        let snap = fb.snapshot(None);
        assert_eq!(std::str::from_utf8(snap.as_bytes()).unwrap(), "cdefghij");
    }
}
