//! Telemetry data types formerly in `nexal-otel`.
//!
//! These are plain data structs and no-op stubs that survived the removal of
//! the full OpenTelemetry pipeline.  They carry no heavy dependencies.

use std::future::Future;
use std::time::Duration;

use serde::Serialize;
use strum_macros::Display;

use crate::ThreadId;
use crate::config_types::ReasoningSummary;
use crate::openai_models::ReasoningEffort;
use crate::protocol::AskForApproval;
use crate::protocol::ReviewDecision;
use crate::protocol::SandboxPolicy;
use crate::protocol::SessionSource;
use crate::user_input::UserInput;

// ---------------------------------------------------------------------------
// RuntimeMetricTotals / RuntimeMetricsSummary
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RuntimeMetricTotals {
    pub count: u64,
    pub duration_ms: u64,
}

impl RuntimeMetricTotals {
    pub fn is_empty(self) -> bool {
        self.count == 0 && self.duration_ms == 0
    }

    pub fn merge(&mut self, other: Self) {
        self.count = self.count.saturating_add(other.count);
        self.duration_ms = self.duration_ms.saturating_add(other.duration_ms);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RuntimeMetricsSummary {
    pub tool_calls: RuntimeMetricTotals,
    pub api_calls: RuntimeMetricTotals,
    pub streaming_events: RuntimeMetricTotals,
    pub websocket_calls: RuntimeMetricTotals,
    pub websocket_events: RuntimeMetricTotals,
    pub responses_api_overhead_ms: u64,
    pub responses_api_inference_time_ms: u64,
    pub responses_api_engine_iapi_ttft_ms: u64,
    pub responses_api_engine_service_ttft_ms: u64,
    pub responses_api_engine_iapi_tbt_ms: u64,
    pub responses_api_engine_service_tbt_ms: u64,
    pub turn_ttft_ms: u64,
    pub turn_ttfm_ms: u64,
}

impl RuntimeMetricsSummary {
    pub fn is_empty(self) -> bool {
        self.tool_calls.is_empty()
            && self.api_calls.is_empty()
            && self.streaming_events.is_empty()
            && self.websocket_calls.is_empty()
            && self.websocket_events.is_empty()
            && self.responses_api_overhead_ms == 0
            && self.responses_api_inference_time_ms == 0
            && self.responses_api_engine_iapi_ttft_ms == 0
            && self.responses_api_engine_service_ttft_ms == 0
            && self.responses_api_engine_iapi_tbt_ms == 0
            && self.responses_api_engine_service_tbt_ms == 0
            && self.turn_ttft_ms == 0
            && self.turn_ttfm_ms == 0
    }

    pub fn merge(&mut self, other: Self) {
        self.tool_calls.merge(other.tool_calls);
        self.api_calls.merge(other.api_calls);
        self.streaming_events.merge(other.streaming_events);
        self.websocket_calls.merge(other.websocket_calls);
        self.websocket_events.merge(other.websocket_events);
        if other.responses_api_overhead_ms > 0 {
            self.responses_api_overhead_ms = other.responses_api_overhead_ms;
        }
        if other.responses_api_inference_time_ms > 0 {
            self.responses_api_inference_time_ms = other.responses_api_inference_time_ms;
        }
        if other.responses_api_engine_iapi_ttft_ms > 0 {
            self.responses_api_engine_iapi_ttft_ms = other.responses_api_engine_iapi_ttft_ms;
        }
        if other.responses_api_engine_service_ttft_ms > 0 {
            self.responses_api_engine_service_ttft_ms = other.responses_api_engine_service_ttft_ms;
        }
        if other.responses_api_engine_iapi_tbt_ms > 0 {
            self.responses_api_engine_iapi_tbt_ms = other.responses_api_engine_iapi_tbt_ms;
        }
        if other.responses_api_engine_service_tbt_ms > 0 {
            self.responses_api_engine_service_tbt_ms = other.responses_api_engine_service_tbt_ms;
        }
        if other.turn_ttft_ms > 0 {
            self.turn_ttft_ms = other.turn_ttft_ms;
        }
        if other.turn_ttfm_ms > 0 {
            self.turn_ttfm_ms = other.turn_ttfm_ms;
        }
    }

    pub fn responses_api_summary(&self) -> RuntimeMetricsSummary {
        Self {
            responses_api_overhead_ms: self.responses_api_overhead_ms,
            responses_api_inference_time_ms: self.responses_api_inference_time_ms,
            responses_api_engine_iapi_ttft_ms: self.responses_api_engine_iapi_ttft_ms,
            responses_api_engine_service_ttft_ms: self.responses_api_engine_service_ttft_ms,
            responses_api_engine_iapi_tbt_ms: self.responses_api_engine_iapi_tbt_ms,
            responses_api_engine_service_tbt_ms: self.responses_api_engine_service_tbt_ms,
            ..RuntimeMetricsSummary::default()
        }
    }
}

// ---------------------------------------------------------------------------
// ToolDecisionSource
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Display)]
#[serde(rename_all = "snake_case")]
pub enum ToolDecisionSource {
    AutomatedReviewer,
    Config,
    User,
}

// ---------------------------------------------------------------------------
// TelemetryAuthMode
// ---------------------------------------------------------------------------

/// Maps to API/auth `AuthMode` to avoid a circular dependency on nexal-core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum TelemetryAuthMode {
    ApiKey,
    Chatgpt,
}

// ---------------------------------------------------------------------------
// AuthEnvTelemetryMetadata
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthEnvTelemetryMetadata {
    pub openai_api_key_env_present: bool,
    pub nexal_api_key_env_present: bool,
    pub nexal_api_key_env_enabled: bool,
    pub provider_env_key_name: Option<String>,
    pub provider_env_key_present: Option<bool>,
    pub refresh_token_url_override_present: bool,
}

// ---------------------------------------------------------------------------
// SessionTelemetryMetadata / SessionTelemetry
// ---------------------------------------------------------------------------

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
}

impl SessionTelemetry {
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
    ) -> Self {
        Self {
            metadata: SessionTelemetryMetadata {
                conversation_id,
                auth_mode: auth_mode.map(|m| m.to_string()),
                auth_env: AuthEnvTelemetryMetadata::default(),
                account_id,
                account_email,
                originator: nexal_utils_string::sanitize_metric_tag_value(originator.as_str()),
                service_name: None,
                session_source: session_source.to_string(),
                model: model.to_owned(),
                slug: slug.to_owned(),
                log_user_prompts,
                app_version: env!("CARGO_PKG_VERSION"),
                terminal_type,
            },
        }
    }

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
        self.metadata.service_name =
            Some(nexal_utils_string::sanitize_metric_tag_value(service_name));
        self
    }

    // -- No-op metric/telemetry stubs kept so call sites compile unchanged --

    pub fn counter(&self, _name: &str, _inc: i64, _tags: &[(&str, &str)]) {
        // no-op
    }

    pub fn histogram(&self, _name: &str, _value: i64, _tags: &[(&str, &str)]) {
        // no-op
    }

    pub fn record_duration(&self, _name: &str, _duration: Duration, _tags: &[(&str, &str)]) {
        // no-op
    }

    pub fn shutdown_metrics(&self) {}

    pub fn reset_runtime_metrics(&self) {
        // no-op
    }

    pub fn runtime_metrics_summary(&self) -> Option<RuntimeMetricsSummary> {
        None
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

    pub fn record_responses<T>(&self, _handle_responses_span: &tracing::Span, _event: &T) {
        // no-op
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

    pub fn record_websocket_event<T>(&self, _result: &T, _duration: Duration) {
        // no-op
    }

    pub fn log_sse_event<T>(&self, _response: &T, _duration: Duration) {
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


    pub fn start_timer(
        &self,
        _name: &str,
        _tags: &[(&str, &str)],
    ) -> Result<Timer, TimerError> {
        Err(TimerError)
    }
}

// ---------------------------------------------------------------------------
// Timer (no-op replacement)
// ---------------------------------------------------------------------------

/// No-op error returned when starting a timer on a disabled exporter.
#[derive(Debug)]
pub struct TimerError;

impl std::fmt::Display for TimerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("metrics exporter is disabled")
    }
}

impl std::error::Error for TimerError {}

/// No-op timer. Drop does nothing.
#[derive(Debug)]
pub struct Timer {
    _private: (),
}

impl Timer {
    /// No-op: record does nothing.
    pub fn record(&self, _additional_tags: &[(&str, &str)]) -> Result<(), TimerError> {
        Ok(())
    }
}
