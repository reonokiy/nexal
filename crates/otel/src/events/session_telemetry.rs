use crate::TelemetryAuthMode;
use crate::ToolDecisionSource;
use crate::metrics::MetricsClient;
use crate::metrics::MetricsConfig;
use crate::metrics::MetricsError;
use crate::metrics::Result as MetricsResult;
use crate::metrics::runtime_metrics::RuntimeMetricsSummary;
use crate::metrics::timer::Timer;
use crate::provider::OtelProvider;
use crate::sanitize_metric_tag_value;
use nexal_api::ApiError;
use nexal_api::ResponseEvent;
use nexal_protocol::ThreadId;
use nexal_protocol::config_types::ReasoningSummary;
use nexal_protocol::openai_models::ReasoningEffort;
use nexal_protocol::protocol::AskForApproval;
use nexal_protocol::protocol::ReviewDecision;
use nexal_protocol::protocol::SandboxPolicy;
use nexal_protocol::protocol::SessionSource;
use nexal_protocol::user_input::UserInput;
use eventsource_stream::Event as StreamEvent;
use eventsource_stream::EventStreamError as StreamError;
use reqwest::Error;
use reqwest::Response;
use std::future::Future;
use std::time::Duration;
use tokio::time::error::Elapsed;
use tracing::Span;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthEnvTelemetryMetadata {
    pub openai_api_key_env_present: bool,
    pub nexal_api_key_env_present: bool,
    pub nexal_api_key_env_enabled: bool,
    pub provider_env_key_name: Option<String>,
    pub provider_env_key_present: Option<bool>,
    pub refresh_token_url_override_present: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SessionTelemetryMetadata {
    pub(crate) conversation_id: ThreadId,
    pub(crate) auth_mode: Option<String>,
    pub(crate) auth_env: AuthEnvTelemetryMetadata,
    pub(crate) account_id: Option<String>,
    pub(crate) account_email: Option<String>,
    pub(crate) originator: String,
    pub(crate) service_name: Option<String>,
    pub(crate) session_source: String,
    pub(crate) model: String,
    pub(crate) slug: String,
    pub(crate) log_user_prompts: bool,
    pub(crate) app_version: &'static str,
    pub(crate) terminal_type: String,
}

#[derive(Debug, Clone)]
pub struct SessionTelemetry {
    pub(crate) metadata: SessionTelemetryMetadata,
    pub(crate) metrics: Option<MetricsClient>,
    pub(crate) metrics_use_metadata_tags: bool,
}

impl SessionTelemetry {
    pub fn with_auth_env(mut self, auth_env: AuthEnvTelemetryMetadata) -> Self {
        self.metadata.auth_env = auth_env;
        self
    }

    pub fn with_model(mut self, model: &str, slug: &str) -> Self {
        self.metadata.model = model.to_owned();
        self.metadata.slug = slug.to_owned();
        self
    }

    pub fn with_metrics_service_name(mut self, service_name: &str) -> Self {
        self.metadata.service_name = Some(sanitize_metric_tag_value(service_name));
        self
    }

    pub fn with_metrics(mut self, metrics: MetricsClient) -> Self {
        self.metrics = Some(metrics);
        self.metrics_use_metadata_tags = true;
        self
    }

    pub fn with_metrics_without_metadata_tags(mut self, metrics: MetricsClient) -> Self {
        self.metrics = Some(metrics);
        self.metrics_use_metadata_tags = false;
        self
    }

    pub fn with_metrics_config(self, _config: MetricsConfig) -> MetricsResult<Self> {
        Ok(self)
    }

    pub fn with_provider_metrics(self, _provider: &OtelProvider) -> Self {
        self
    }

    pub fn counter(&self, _name: &str, _inc: i64, _tags: &[(&str, &str)]) {
        // no-op
    }

    pub fn histogram(&self, _name: &str, _value: i64, _tags: &[(&str, &str)]) {
        // no-op
    }

    pub fn record_duration(&self, _name: &str, _duration: Duration, _tags: &[(&str, &str)]) {
        // no-op
    }

    pub fn start_timer(&self, _name: &str, _tags: &[(&str, &str)]) -> Result<Timer, MetricsError> {
        Err(MetricsError::ExporterDisabled)
    }

    pub fn shutdown_metrics(&self) -> MetricsResult<()> {
        Ok(())
    }

    pub fn snapshot_metrics(&self) -> MetricsResult<()> {
        Err(MetricsError::ExporterDisabled)
    }

    /// No-op: collect and discard a runtime metrics snapshot.
    pub fn reset_runtime_metrics(&self) {
        // no-op
    }

    /// No-op: always returns None.
    pub fn runtime_metrics_summary(&self) -> Option<RuntimeMetricsSummary> {
        None
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        conversation_id: ThreadId,
        model: &str,
        slug: &str,
        account_id: Option<String>,
        account_email: Option<String>,
        auth_mode: Option<TelemetryAuthMode>,
        originator: String,
        log_user_prompts: bool,
        terminal_type: String,
        session_source: SessionSource,
    ) -> SessionTelemetry {
        Self {
            metadata: SessionTelemetryMetadata {
                conversation_id,
                auth_mode: auth_mode.map(|m| m.to_string()),
                auth_env: AuthEnvTelemetryMetadata::default(),
                account_id,
                account_email,
                originator: sanitize_metric_tag_value(originator.as_str()),
                service_name: None,
                session_source: session_source.to_string(),
                model: model.to_owned(),
                slug: slug.to_owned(),
                log_user_prompts,
                app_version: env!("CARGO_PKG_VERSION"),
                terminal_type,
            },
            metrics: None,
            metrics_use_metadata_tags: true,
        }
    }

    pub fn record_responses(&self, _handle_responses_span: &Span, _event: &ResponseEvent) {
        // no-op
    }

    #[allow(clippy::too_many_arguments)]
    pub fn conversation_starts(
        &self,
        _provider_name: &str,
        _reasoning_effort: Option<ReasoningEffort>,
        _reasoning_summary: ReasoningSummary,
        _context_window: Option<i64>,
        _auto_compact_token_limit: Option<i64>,
        _approval_policy: AskForApproval,
        _sandbox_policy: SandboxPolicy,
        _mcp_servers: Vec<&str>,
        _active_profile: Option<String>,
    ) {
        // no-op
    }

    pub async fn log_request<F, Fut>(&self, _attempt: u64, f: F) -> Result<Response, Error>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Response, Error>>,
    {
        f().await
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_api_request(
        &self,
        _attempt: u64,
        _status: Option<u16>,
        _error: Option<&str>,
        _duration: Duration,
        _auth_header_attached: bool,
        _auth_header_name: Option<&str>,
        _retry_after_unauthorized: bool,
        _recovery_mode: Option<&str>,
        _recovery_phase: Option<&str>,
        _endpoint: &str,
        _request_id: Option<&str>,
        _cf_ray: Option<&str>,
        _auth_error: Option<&str>,
        _auth_error_code: Option<&str>,
    ) {
        // no-op
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_websocket_connect(
        &self,
        _duration: Duration,
        _status: Option<u16>,
        _error: Option<&str>,
        _auth_header_attached: bool,
        _auth_header_name: Option<&str>,
        _retry_after_unauthorized: bool,
        _recovery_mode: Option<&str>,
        _recovery_phase: Option<&str>,
        _endpoint: &str,
        _connection_reused: bool,
        _request_id: Option<&str>,
        _cf_ray: Option<&str>,
        _auth_error: Option<&str>,
        _auth_error_code: Option<&str>,
    ) {
        // no-op
    }

    pub fn record_websocket_request(
        &self,
        _duration: Duration,
        _error: Option<&str>,
        _connection_reused: bool,
    ) {
        // no-op
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_auth_recovery(
        &self,
        _mode: &str,
        _step: &str,
        _outcome: &str,
        _request_id: Option<&str>,
        _cf_ray: Option<&str>,
        _auth_error: Option<&str>,
        _auth_error_code: Option<&str>,
        _recovery_reason: Option<&str>,
        _auth_state_changed: Option<bool>,
    ) {
        // no-op
    }

    pub fn record_websocket_event(
        &self,
        _result: &Result<
            Option<
                Result<
                    tokio_tungstenite::tungstenite::Message,
                    tokio_tungstenite::tungstenite::Error,
                >,
            >,
            ApiError,
        >,
        _duration: Duration,
    ) {
        // no-op
    }

    pub fn log_sse_event<E>(
        &self,
        _response: &Result<Option<Result<StreamEvent, StreamError<E>>>, Elapsed>,
        _duration: Duration,
    ) where
        E: std::fmt::Display,
    {
        // no-op
    }

    pub fn sse_event_failed<T>(&self, _kind: Option<&String>, _duration: Duration, _error: &T)
    where
        T: std::fmt::Display,
    {
        // no-op
    }

    pub fn see_event_completed_failed<T>(&self, _error: &T)
    where
        T: std::fmt::Display,
    {
        // no-op
    }

    pub fn sse_event_completed(
        &self,
        _input_token_count: i64,
        _output_token_count: i64,
        _cached_token_count: Option<i64>,
        _reasoning_token_count: Option<i64>,
        _tool_token_count: i64,
    ) {
        // no-op
    }

    pub fn user_prompt(&self, _items: &[UserInput]) {
        // no-op
    }

    pub fn tool_decision(
        &self,
        _tool_name: &str,
        _call_id: &str,
        _decision: &ReviewDecision,
        _source: ToolDecisionSource,
    ) {
        // no-op
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn log_tool_result_with_tags<F, Fut, E>(
        &self,
        _tool_name: &str,
        _call_id: &str,
        _arguments: &str,
        _extra_tags: &[(&str, &str)],
        _mcp_server: Option<&str>,
        _mcp_server_origin: Option<&str>,
        f: F,
    ) -> Result<(String, bool), E>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(String, bool), E>>,
        E: std::fmt::Display,
    {
        f().await
    }

    pub fn log_tool_failed(&self, _tool_name: &str, _error: &str) {
        // no-op
    }

    #[allow(clippy::too_many_arguments)]
    pub fn tool_result_with_tags(
        &self,
        _tool_name: &str,
        _call_id: &str,
        _arguments: &str,
        _duration: Duration,
        _success: bool,
        _output: &str,
        _extra_tags: &[(&str, &str)],
        _mcp_server: Option<&str>,
        _mcp_server_origin: Option<&str>,
    ) {
        // no-op
    }
}
