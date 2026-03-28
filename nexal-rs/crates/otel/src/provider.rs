use crate::config::OtelSettings;
use crate::metrics::MetricsClient;
use std::error::Error;
use tracing_subscriber::Layer;
use tracing_subscriber::registry::LookupSpan;

/// No-op OpenTelemetry provider. All methods are stubs.
pub struct OtelProvider {
    pub metrics: Option<MetricsClient>,
}

impl OtelProvider {
    pub fn shutdown(&self) {
        // no-op
    }

    /// No-op: always returns Ok(None).
    pub fn from(_settings: &OtelSettings) -> Result<Option<Self>, Box<dyn Error>> {
        Ok(None)
    }

    /// No-op: always returns None.
    pub fn logger_layer<S>(&self) -> Option<NoopLayer<S>>
    where
        S: tracing::Subscriber + for<'span> LookupSpan<'span> + Send + Sync,
    {
        None
    }

    /// No-op: always returns None.
    pub fn tracing_layer<S>(&self) -> Option<NoopLayer<S>>
    where
        S: tracing::Subscriber + for<'span> LookupSpan<'span> + Send + Sync,
    {
        None
    }

    pub fn nexal_export_filter(_meta: &tracing::Metadata<'_>) -> bool {
        false
    }

    pub fn log_export_filter(_meta: &tracing::Metadata<'_>) -> bool {
        false
    }

    pub fn trace_export_filter(_meta: &tracing::Metadata<'_>) -> bool {
        false
    }

    pub fn metrics(&self) -> Option<&MetricsClient> {
        self.metrics.as_ref()
    }
}

impl Drop for OtelProvider {
    fn drop(&mut self) {
        // no-op
    }
}

/// A trivial layer that does nothing, used so `logger_layer` / `tracing_layer`
/// have a concrete `impl Layer` to return (though they always return `None`).
pub struct NoopLayer<S>(std::marker::PhantomData<S>);

impl<S> Layer<S> for NoopLayer<S> where S: tracing::Subscriber {}
