pub(crate) mod jsonrpsee_api;
pub(crate) mod protocol;
pub(crate) mod rpc;

#[derive(Debug, thiserror::Error)]
pub enum ExecServerError {
    #[error("exec-server protocol error: {0}")]
    Protocol(String),
    #[error("exec-server rejected request ({code}): {message}")]
    Server { code: i64, message: String },
}
