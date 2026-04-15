//! Frontend ↔ gateway JSON-RPC types.
//!
//! Wire format is JSON-RPC 2.0 over a single WebSocket. We use raw
//! `serde_json::Value` for `id` and inner request/response payloads so
//! that we can transparently forward agent-bound traffic without
//! having to model every possible agent method here.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const JSONRPC_VERSION: &str = "2.0";

// ── Method names ─────────────────────────────────────────────────────

pub const METHOD_HELLO: &str = "gateway/hello";
pub const METHOD_SPAWN_AGENT: &str = "gateway/spawnAgent";
pub const METHOD_KILL_AGENT: &str = "gateway/killAgent";
pub const METHOD_DETACH_AGENT: &str = "gateway/detachAgent";
pub const METHOD_ATTACH_AGENT: &str = "gateway/attachAgent";
pub const METHOD_LIST_AGENTS: &str = "gateway/listAgents";
pub const METHOD_AGENT_INVOKE: &str = "agent/invoke";

/// Notification carrying an in-band notification from a specific agent.
pub const NOTIFY_AGENT: &str = "agent/notify";

// ── JSON-RPC envelope ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    /// `None` means notification (no response expected).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcResponse {
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

pub fn notification(method: &str, params: Value) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: JSONRPC_VERSION.into(),
        id: None,
        method: method.into(),
        params: Some(params),
    }
}

// ── Standard error codes ─────────────────────────────────────────────

pub mod error_code {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;

    /// gateway/hello not yet completed.
    pub const NOT_AUTHENTICATED: i32 = -32000;
    /// Wrong / missing auth token.
    pub const AUTH_REJECTED: i32 = -32001;
    /// Specified agentId does not exist.
    pub const UNKNOWN_AGENT: i32 = -32010;
    /// Backend error (podman, sandbox, …).
    pub const BACKEND_ERROR: i32 = -32020;
}

// ── gateway/hello ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloParams {
    pub token: String,
    pub client_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloResponse {
    pub ok: bool,
    pub gateway_version: String,
}

// ── gateway/spawnAgent ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnAgentParams {
    /// Human-friendly suffix for the container name (e.g. `worker-abc`).
    pub name: String,
    /// Image override (falls back to gateway default).
    #[serde(default)]
    pub image: Option<String>,
    /// Extra env vars passed into the container.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Extra labels merged with the default `app=nexal` set.
    #[serde(default)]
    pub labels: HashMap<String, String>,
    /// Optional host workspace bind-mounted at `/workspace` inside the container.
    #[serde(default)]
    pub workspace: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnAgentResponse {
    pub agent_id: String,
    pub container_name: String,
}

// ── gateway/killAgent / detachAgent ──────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentIdParams {
    pub agent_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OkResponse {
    pub ok: bool,
}

// ── gateway/attachAgent ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachAgentParams {
    pub container_name: String,
}

// ── gateway/listAgents ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListAgentsResponse {
    pub agents: Vec<AgentSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSummary {
    pub agent_id: String,
    pub container_name: String,
    pub created_at_unix_ms: u64,
}

// ── agent/invoke ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInvokeParams {
    pub agent_id: String,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

// agent/notify wraps a notification coming from an agent
// (e.g. process/output, process/exited).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentNotifyParams {
    pub agent_id: String,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}
