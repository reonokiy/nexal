//! Crate-wide error type.

/// Errors produced by the exec-server when handling requests.
#[derive(Debug, thiserror::Error)]
pub enum ExecServerError {
    #[error("exec-server protocol error: {0}")]
    Protocol(String),
    #[error("exec-server rejected request ({kind}): {message}")]
    Server { kind: String, message: String },
}
