use nexal_protocol::protocol::W3cTraceContext;
use tracing::Span;

/// Opaque no-op context. Substitutes for `opentelemetry::Context`.
#[derive(Clone)]
pub struct Context;

/// No-op: always returns None.
pub fn current_span_w3c_trace_context() -> Option<W3cTraceContext> {
    None
}

/// No-op: always returns None.
pub fn span_w3c_trace_context(_span: &Span) -> Option<W3cTraceContext> {
    None
}

/// No-op: always returns None.
pub fn current_span_trace_id() -> Option<String> {
    None
}

/// No-op: always returns None.
pub fn context_from_w3c_trace_context(_trace: &W3cTraceContext) -> Option<Context> {
    None
}

/// No-op: always returns false.
pub fn set_parent_from_w3c_trace_context(_span: &Span, _trace: &W3cTraceContext) -> bool {
    false
}

/// No-op.
pub fn set_parent_from_context(_span: &Span, _context: Context) {
    // no-op
}

/// No-op: always returns None.
pub fn traceparent_context_from_env() -> Option<Context> {
    None
}
