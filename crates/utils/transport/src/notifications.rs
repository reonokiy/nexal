use serde::{Deserialize, Serialize};

use crate::agent::StreamKind;

pub const PROCESS_OUTPUT: &str = "process/output";
pub const PROCESS_EXITED: &str = "process/exited";
pub const PROCESS_CLOSED: &str = "process/closed";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProcessOutputNotification {
    pub process_id: String,
    pub seq: u64,
    pub stream: StreamKind,
    #[serde(with = "serde_bytes")]
    pub chunk: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProcessExitedNotification {
    pub process_id: String,
    pub seq: u64,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProcessClosedNotification {
    pub process_id: String,
    pub seq: u64,
}
