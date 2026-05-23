//! Channel error types.

use rmpv::Value;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, PartialOrd, Ord, Deserialize, Serialize, Hash, Eq)]
#[serde(untagged)]
pub enum RequestId {
    String(String),
    Integer(i64),
    /// Null ids are used on error responses for malformed frames where
    /// the original id could not be parsed.
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelErrorKind {
    Parse,
    InvalidRequest,
    MethodNotFound,
    InvalidParams,
    Internal,
}

impl std::fmt::Display for ChannelErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Parse => "parse",
            Self::InvalidRequest => "invalid_request",
            Self::MethodNotFound => "method_not_found",
            Self::InvalidParams => "invalid_params",
            Self::Internal => "internal",
        };
        f.write_str(s)
    }
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String(value) => f.write_str(value),
            Self::Integer(value) => write!(f, "{value}"),
            Self::Null => f.write_str("null"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ChannelError {
    pub kind: ChannelErrorKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    pub message: String,
}
