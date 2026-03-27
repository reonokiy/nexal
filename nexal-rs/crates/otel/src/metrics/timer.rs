use crate::metrics::MetricsClient;
use crate::metrics::error::Result;

/// No-op timer. Drop does nothing.
#[derive(Debug)]
pub struct Timer {
    _name: String,
    _tags: Vec<(String, String)>,
    _client: MetricsClient,
}

impl Timer {
    pub(crate) fn new(name: &str, tags: &[(&str, &str)], client: &MetricsClient) -> Self {
        Self {
            _name: name.to_string(),
            _tags: tags
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            _client: client.clone(),
        }
    }

    /// No-op: record does nothing.
    pub fn record(&self, _additional_tags: &[(&str, &str)]) -> Result<()> {
        Ok(())
    }
}
