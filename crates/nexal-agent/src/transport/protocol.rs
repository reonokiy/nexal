use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use crate::ProcessId;

// ── JSON-RPC version marker ────────────────────────────────────────

/// Always serializes to `"2.0"`, deserializes only from `"2.0"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JsonRpcVersion;

impl Serialize for JsonRpcVersion {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str("2.0")
    }
}

impl<'de> Deserialize<'de> for JsonRpcVersion {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        if s == "2.0" {
            Ok(JsonRpcVersion)
        } else {
            Err(serde::de::Error::custom(format!(
                "expected jsonrpc \"2.0\", got {s:?}"
            )))
        }
    }
}

pub const EXEC_OUTPUT_DELTA_METHOD: &str = "process/output";
pub const EXEC_EXITED_METHOD: &str = "process/exited";
pub const EXEC_CLOSED_METHOD: &str = "process/closed";

// ── Request method enum ────────────────────────────────────────────

/// Parsed JSON-RPC method name, grouped by domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Lifecycle(LifecycleMethod),
    Process(ProcessMethod),
    FileSystem(FsMethod),
    Proxy(ProxyMethod),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleMethod {
    Initialize,
    Initialized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessMethod {
    Start,
    Read,
    Write,
    Terminate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsMethod {
    ReadFile,
    WriteFile,
    CreateDirectory,
    GetMetadata,
    ReadDirectory,
    Remove,
    Copy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyMethod {
    Register,
    Unregister,
}

/// Parse a wire-format method name into a [`Method`].
pub fn parse_method(s: &str) -> Option<Method> {
    match s {
        "initialize" => Some(Method::Lifecycle(LifecycleMethod::Initialize)),
        "initialized" => Some(Method::Lifecycle(LifecycleMethod::Initialized)),
        "process/start" => Some(Method::Process(ProcessMethod::Start)),
        "process/read" => Some(Method::Process(ProcessMethod::Read)),
        "process/write" => Some(Method::Process(ProcessMethod::Write)),
        "process/terminate" => Some(Method::Process(ProcessMethod::Terminate)),
        "fs/read_file" => Some(Method::FileSystem(FsMethod::ReadFile)),
        "fs/write_file" => Some(Method::FileSystem(FsMethod::WriteFile)),
        "fs/create_directory" => Some(Method::FileSystem(FsMethod::CreateDirectory)),
        "fs/get_metadata" => Some(Method::FileSystem(FsMethod::GetMetadata)),
        "fs/read_directory" => Some(Method::FileSystem(FsMethod::ReadDirectory)),
        "fs/remove" => Some(Method::FileSystem(FsMethod::Remove)),
        "fs/copy" => Some(Method::FileSystem(FsMethod::Copy)),
        "proxy/register" => Some(Method::Proxy(ProxyMethod::Register)),
        "proxy/unregister" => Some(Method::Proxy(ProxyMethod::Unregister)),
        _ => None,
    }
}

// ── JSON-RPC error codes ──────────────────────────────────────────

pub const ERROR_CODE_PARSE: i64 = -32700;
pub const ERROR_CODE_INVALID_REQUEST: i64 = -32600;
pub const ERROR_CODE_METHOD_NOT_FOUND: i64 = -32601;
pub const ERROR_CODE_INVALID_PARAMS: i64 = -32602;
pub const ERROR_CODE_INTERNAL: i64 = -32603;

/// Register a reverse proxy Unix socket inside the container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProxyRegisterParams {
    /// Path to the Unix socket (e.g. "/workspace/agents/proxy/api.telegram.org").
    pub socket_path: String,
    /// Upstream URL to forward requests to (e.g. "https://api.telegram.org").
    pub upstream_url: String,
    /// Headers to inject into every proxied request (e.g. auth tokens).
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProxyRegisterResponse {
    pub ok: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProxyUnregisterParams {
    pub socket_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProxyUnregisterResponse {
    pub ok: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InitializeParams {
    pub client_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InitializeResponse {
    /// Default shell available in this execution environment (e.g. "/bin/bash").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_shell: Option<String>,
    /// Working directory of the execution environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExecParams {
    /// Client-chosen logical process handle scoped to this connection/session.
    /// This is a protocol key, not an OS pid.
    pub process_id: ProcessId,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub env: HashMap<String, String>,
    pub tty: bool,
    pub arg0: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExecResponse {
    pub process_id: ProcessId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReadParams {
    pub process_id: ProcessId,
    pub after_seq: Option<u64>,
    pub max_bytes: Option<usize>,
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProcessOutputChunk {
    pub seq: u64,
    pub stream: ExecOutputStream,
    pub chunk: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReadResponse {
    pub chunks: Vec<ProcessOutputChunk>,
    pub next_seq: u64,
    pub exited: bool,
    pub exit_code: Option<i32>,
    pub closed: bool,
    pub failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WriteParams {
    pub process_id: ProcessId,
    pub chunk: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteStatus {
    Accepted,
    UnknownProcess,
    StdinClosed,
    Starting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WriteResponse {
    pub status: WriteStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TerminateParams {
    pub process_id: ProcessId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TerminateResponse {
    pub running: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecOutputStream {
    Stdout,
    Stderr,
    Pty,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExecOutputDeltaNotification {
    pub process_id: ProcessId,
    pub seq: u64,
    pub stream: ExecOutputStream,
    pub chunk: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExecExitedNotification {
    pub process_id: ProcessId,
    pub seq: u64,
    pub exit_code: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExecClosedNotification {
    pub process_id: ProcessId,
    pub seq: u64,
}

// ── JSONRPC envelope types ──

#[derive(Debug, Clone, PartialEq, PartialOrd, Ord, Deserialize, Serialize, Hash, Eq)]
#[serde(untagged)]
pub enum RequestId {
    String(String),
    Integer(i64),
    /// JSON-RPC 2.0 allows `null` ids on error responses for
    /// malformed frames where the original id could not be parsed.
    Null,
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
#[serde(untagged)]
pub enum JSONRPCMessage {
    Request(JSONRPCRequest),
    Notification(JSONRPCNotification),
    Response(JSONRPCResponse),
    Error(JSONRPCError),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct JSONRPCRequest {
    #[serde(default = "jsonrpc_2_0")]
    pub jsonrpc: JsonRpcVersion,
    pub id: RequestId,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<rmpv::Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct JSONRPCNotification {
    #[serde(default = "jsonrpc_2_0")]
    pub jsonrpc: JsonRpcVersion,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<rmpv::Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct JSONRPCResponse {
    #[serde(default = "jsonrpc_2_0")]
    pub jsonrpc: JsonRpcVersion,
    pub id: RequestId,
    pub result: rmpv::Value,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct JSONRPCError {
    #[serde(default = "jsonrpc_2_0")]
    pub jsonrpc: JsonRpcVersion,
    pub error: JSONRPCErrorError,
    pub id: RequestId,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct JSONRPCErrorError {
    pub code: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<rmpv::Value>,
    pub message: String,
}

fn jsonrpc_2_0() -> JsonRpcVersion {
    JsonRpcVersion
}

// ── Filesystem wire types ──

use nexal_utils_absolute_path::AbsolutePathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsReadFileParams {
    pub path: AbsolutePathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsReadFileResponse {
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsWriteFileParams {
    pub path: AbsolutePathBuf,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsWriteFileResponse {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsCreateDirectoryParams {
    pub path: AbsolutePathBuf,
    pub recursive: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsCreateDirectoryResponse {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsGetMetadataParams {
    pub path: AbsolutePathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsGetMetadataResponse {
    pub is_directory: bool,
    pub is_file: bool,
    pub created_at_ms: i64,
    pub modified_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsReadDirectoryParams {
    pub path: AbsolutePathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsReadDirectoryEntry {
    pub file_name: String,
    pub is_directory: bool,
    pub is_file: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsReadDirectoryResponse {
    pub entries: Vec<FsReadDirectoryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsRemoveParams {
    pub path: AbsolutePathBuf,
    pub recursive: Option<bool>,
    pub force: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsRemoveResponse {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsCopyParams {
    pub source_path: AbsolutePathBuf,
    pub destination_path: AbsolutePathBuf,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub recursive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsCopyResponse {}

#[cfg(test)]
mod tests {
    use super::*;

    fn msgpack_roundtrip<T: Serialize + serde::de::DeserializeOwned + std::fmt::Debug + PartialEq>(
        val: &T,
    ) {
        let v = rmpv::ext::to_value(val).expect("serialize");
        let back: T = rmpv::ext::from_value(v).expect("deserialize");
        assert_eq!(back, *val);
    }

    // ── RequestId round-trip ──────────────────────────────────────

    #[test]
    fn request_id_string_round_trips() {
        let id = RequestId::String("abc-123".into());
        msgpack_roundtrip(&id);
    }

    #[test]
    fn request_id_integer_round_trips() {
        let id = RequestId::Integer(42);
        msgpack_roundtrip(&id);
    }

    #[test]
    fn request_id_null_round_trips() {
        let id = RequestId::Null;
        msgpack_roundtrip(&id);
    }

    #[test]
    fn request_id_display_formats_correctly() {
        assert_eq!(RequestId::String("x".into()).to_string(), "x");
        assert_eq!(RequestId::Integer(99).to_string(), "99");
        assert_eq!(RequestId::Null.to_string(), "null");
    }
}
