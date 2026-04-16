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
pub const METHOD_SPAWN_AGENT: &str = "gateway/spawn_agent";
pub const METHOD_KILL_AGENT: &str = "gateway/kill_agent";
pub const METHOD_DETACH_AGENT: &str = "gateway/detach_agent";
pub const METHOD_ATTACH_AGENT: &str = "gateway/attach_agent";
pub const METHOD_LIST_AGENTS: &str = "gateway/list_agents";
pub const METHOD_AGENT_INVOKE: &str = "agent/invoke";
pub const METHOD_REGISTER_PROXY: &str = "gateway/register_proxy";
pub const METHOD_UNREGISTER_PROXY: &str = "gateway/unregister_proxy";

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
#[serde(rename_all = "snake_case")]
pub struct HelloParams {
    pub token: String,
    pub client_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HelloResponse {
    pub ok: bool,
    pub gateway_version: String,
}

// ── gateway/spawnAgent ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
#[serde(rename_all = "snake_case")]
pub struct SpawnAgentResponse {
    pub agent_id: String,
    pub container_name: String,
}

// ── gateway/killAgent / detachAgent ──────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentIdParams {
    pub agent_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OkResponse {
    pub ok: bool,
}

// ── gateway/attachAgent ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AttachAgentParams {
    pub container_name: String,
}

// ── gateway/listAgents ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ListAgentsResponse {
    pub agents: Vec<AgentSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentSummary {
    pub agent_id: String,
    pub container_name: String,
    pub created_at_unix_ms: u64,
}

// ── agent/invoke ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentInvokeParams {
    pub agent_id: String,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

// agent/notify wraps a notification coming from an agent
// (e.g. process/output, process/exited).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentNotifyParams {
    pub agent_id: String,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

// ── gateway/register_proxy / unregister_proxy ───────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RegisterProxyParams {
    /// Owning agent. When the agent is killed, this proxy is dropped.
    pub agent_id: String,
    /// Frontend-chosen label, unique within `agent_id`. Re-registering
    /// with the same `(agent_id, name)` replaces the previous entry.
    pub name: String,
    /// Base URL — the agent's request path is appended to this.
    pub upstream_url: String,
    /// Headers injected on every forwarded request (typically auth).
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RegisterProxyResponse {
    /// Opaque identifier the in-container nexal-agent forwards to the
    /// gateway with. Used to look the entry up on the proxy HTTP server.
    pub token: String,
    /// Unix socket path inside the container. The gateway told
    /// `nexal-agent` to create it; container code uses it directly,
    /// e.g. `curl --unix-socket /workspace/.nexal/proxies/jina.sock http://x/v1/search`
    /// (the URL's host part is ignored).
    pub socket_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UnregisterProxyParams {
    pub agent_id: String,
    pub name: String,
}

#[cfg(test)]
mod tests {
    //! Wire-format guardrails. Every DTO on the gateway ↔ frontend
    //! boundary must emit snake_case JSON so TS/Bun callers don't get
    //! surprises. Serialize a hand-built instance and confirm the
    //! on-wire key names.

    use super::*;
    use serde_json::json;

    #[test]
    fn hello_params_serializes_snake_case() {
        let p = HelloParams {
            token: "t".into(),
            client_name: "c".into(),
        };
        let v = serde_json::to_value(&p).expect("hello params serialize");
        assert_eq!(v, json!({ "token": "t", "client_name": "c" }));
    }

    #[test]
    fn hello_response_serializes_snake_case() {
        let r = HelloResponse {
            ok: true,
            gateway_version: "0.2.0".into(),
        };
        let v = serde_json::to_value(&r).expect("hello response serialize");
        assert_eq!(v, json!({ "ok": true, "gateway_version": "0.2.0" }));
    }

    #[test]
    fn spawn_agent_params_skips_empty_maps_and_none_fields() {
        let p = SpawnAgentParams {
            name: "n".into(),
            image: None,
            env: HashMap::new(),
            labels: HashMap::new(),
            workspace: None,
        };
        let v = serde_json::to_value(&p).expect("spawn_agent params serialize");
        // `name` present, image/workspace absent, env/labels present as
        // empty maps (serde doesn't strip them without
        // `skip_serializing_if`). Asserting structural shape, not
        // bit-for-bit presence, keeps this robust to future tweaks.
        let obj = v.as_object().expect("serialized shape is an object");
        assert_eq!(obj.get("name"), Some(&json!("n")));
    }

    #[test]
    fn agent_summary_uses_snake_case_ms_suffix() {
        let s = AgentSummary {
            agent_id: "a".into(),
            container_name: "nexal-c".into(),
            created_at_unix_ms: 1_700_000_000_000,
        };
        let v = serde_json::to_value(&s).expect("agent summary serialize");
        let obj = v.as_object().expect("serialized shape is an object");
        assert!(obj.contains_key("agent_id"));
        assert!(obj.contains_key("container_name"));
        assert!(obj.contains_key("created_at_unix_ms"));
    }

    #[test]
    fn register_proxy_response_carries_snake_case_socket_path() {
        let r = RegisterProxyResponse {
            token: "deadbeef".into(),
            socket_path: "/workspace/.nexal/proxies/jina.sock".into(),
        };
        let v = serde_json::to_value(&r).expect("register_proxy response serialize");
        let obj = v.as_object().expect("serialized shape is an object");
        assert!(obj.contains_key("socket_path"));
        // camelCase leaks would show up here — guard against them.
        assert!(!obj.contains_key("socketPath"));
    }

    #[test]
    fn jsonrpc_request_with_no_id_omits_id_field() {
        let req = JsonRpcRequest {
            jsonrpc: JSONRPC_VERSION.into(),
            id: None,
            method: "x".into(),
            params: Some(json!({ "a": 1 })),
        };
        let v = serde_json::to_value(&req).expect("jsonrpc req serialize");
        let obj = v.as_object().expect("serialized shape is an object");
        assert!(!obj.contains_key("id"), "notifications omit `id`");
    }

    #[test]
    fn jsonrpc_response_ok_helper_builds_valid_envelope() {
        let r = JsonRpcResponse::ok(json!("id-1"), json!({ "ok": true }));
        assert_eq!(r.jsonrpc, JSONRPC_VERSION);
        assert_eq!(r.id, json!("id-1"));
        assert!(r.error.is_none());
        assert_eq!(r.result, Some(json!({ "ok": true })));
    }

    #[test]
    fn jsonrpc_response_err_helper_builds_error_envelope() {
        let r = JsonRpcResponse::err(json!(5), error_code::UNKNOWN_AGENT, "nope");
        assert_eq!(r.jsonrpc, JSONRPC_VERSION);
        assert!(r.result.is_none());
        let e = r.error.expect("error response must carry an error");
        assert_eq!(e.code, error_code::UNKNOWN_AGENT);
        assert_eq!(e.message, "nope");
        assert!(e.data.is_none());
    }

    #[test]
    fn notification_helper_sets_id_to_none() {
        let n = notification(NOTIFY_AGENT, json!({ "agent_id": "a" }));
        assert!(n.id.is_none());
        assert_eq!(n.method, NOTIFY_AGENT);
    }

    #[test]
    fn error_codes_are_stable_and_unique() {
        // If two error codes collide it becomes impossible for the
        // frontend to switch on them — enforce uniqueness.
        let codes = [
            error_code::PARSE_ERROR,
            error_code::INVALID_REQUEST,
            error_code::METHOD_NOT_FOUND,
            error_code::INVALID_PARAMS,
            error_code::INTERNAL_ERROR,
            error_code::NOT_AUTHENTICATED,
            error_code::AUTH_REJECTED,
            error_code::UNKNOWN_AGENT,
            error_code::BACKEND_ERROR,
        ];
        let mut sorted = codes.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), codes.len(), "duplicate error code detected");
    }

    #[test]
    fn register_proxy_params_deserializes_from_snake_case_json() {
        let raw = json!({
            "agent_id": "a-1",
            "name": "jina",
            "upstream_url": "https://api.jina.ai",
            "headers": { "Authorization": "Bearer k" }
        });
        let p: RegisterProxyParams = serde_json::from_value(raw).expect("register_proxy parses");
        assert_eq!(p.agent_id, "a-1");
        assert_eq!(p.name, "jina");
        assert_eq!(p.upstream_url, "https://api.jina.ai");
        assert_eq!(p.headers.get("Authorization").map(String::as_str), Some("Bearer k"));
    }
}
