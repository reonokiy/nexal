use crate::metrics::MetricsError;
use crate::metrics::Result;
use crate::metrics::config::MetricsConfig;
use crate::metrics::timer::Timer;
use std::time::Duration;

/// No-op OpenTelemetry metrics client.
#[derive(Clone, Debug)]
pub struct MetricsClient;

impl MetricsClient {
    /// Build a no-op metrics client. Config is accepted but ignored.
    pub fn new(_config: MetricsConfig) -> Result<Self> {
        Ok(Self)
    }

    /// No-op: counter increment.
    pub fn counter(&self, _name: &str, _inc: i64, _tags: &[(&str, &str)]) -> Result<()> {
        Ok(())
    }

    /// No-op: histogram sample.
    pub fn histogram(&self, _name: &str, _value: i64, _tags: &[(&str, &str)]) -> Result<()> {
        Ok(())
    }

    /// No-op: record a duration histogram.
    pub fn record_duration(
        &self,
        _name: &str,
        _duration: Duration,
        _tags: &[(&str, &str)],
    ) -> Result<()> {
        Ok(())
    }

    pub fn start_timer(
        &self,
        name: &str,
        tags: &[(&str, &str)],
    ) -> std::result::Result<Timer, MetricsError> {
        Ok(Timer::new(name, tags, self))
    }

    /// No-op: always returns ExporterDisabled.
    pub fn snapshot(&self) -> Result<()> {
        Err(MetricsError::ExporterDisabled)
    }

    /// No-op: shutdown.
    pub fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}
