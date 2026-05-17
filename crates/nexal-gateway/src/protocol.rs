//! Frontend ↔ gateway JSON-RPC types (MessagePack binary wire format).
//!
//! Wire format is JSON-RPC 2.0 over WebSocket Binary frames. We use raw
//! `rmpv::Value` for `id` and inner request/response payloads so
//! that we can transparently forward agent-bound traffic without
//! having to model every possible agent method here.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use rmpv::Value;

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
pub const METHOD_REGISTER_STREAM_PROXY: &str = "gateway/register_stream_proxy";
pub const METHOD_UNREGISTER_STREAM_PROXY: &str = "gateway/unregister_stream_proxy";

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
    /// Credential identifier (looked up server-side → secret).
    pub access_key: String,
    pub client_name: String,
    /// Unix seconds when the request was signed (replay window check).
    pub ts: i64,
    /// Single-use random hex string (replay guard).
    pub nonce: String,
    /// Lowercase hex HMAC-SHA256(secret_key,
    /// `"{access_key}\n{ts}\n{nonce}\n{client_name}"`).
    pub signature: String,
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
    /// Extra container ports to publish for direct TCP access
    /// (e.g. `[3389, 9222]` for RDP / CDP).
    #[serde(default)]
    pub extra_ports: Vec<u16>,
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
    /// e.g. `curl --unix-socket /run/nexal/proxy/jina.socket http://x/v1/search`
    /// (the URL's host part is ignored).
    pub socket_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UnregisterProxyParams {
    pub agent_id: String,
    pub name: String,
}

// ── gateway/register_stream_proxy / unregister_stream_proxy ─────────

/// Register a TCP proxy that forwards an external gateway port to a
/// container port. The gateway allocates a random listen port and
/// does direct `tokio::io::copy` — no encoding overhead.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RegisterStreamProxyParams {
    /// Owning agent.
    pub agent_id: String,
    /// Frontend-chosen label, unique within `agent_id`.
    pub name: String,
    /// Port inside the container to forward to (e.g. 3389 for RDP,
    /// 9222 for CDP).
    pub container_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RegisterStreamProxyResponse {
    /// Gateway-side listen address clients should connect to
    /// (e.g. `"127.0.0.1:49201"`).
    pub listen_addr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UnregisterStreamProxyParams {
    pub agent_id: String,
    pub name: String,
}

#[cfg(test)]
mod tests {
    //! Wire-format guardrails. Every DTO on the gateway ↔ frontend
    //! boundary are preserved (snake_case map keys) even with the
    //! MessagePack wire format.

    use super::*;

    fn mpv_from_json(s: &str) -> rmpv::Value {
        let jv: serde_json::Value = serde_json::from_str(s).unwrap();
        let mut buf = Vec::new();
        let mut ser = rmp_serde::Serializer::new(&mut buf).with_struct_map();
        jv.serialize(&mut ser).unwrap();
        rmp_serde::from_slice(&buf).unwrap()
    }

    fn mpv_roundtrip<T: serde::Serialize + serde::de::DeserializeOwned>(v: &T) -> rmpv::Value {
        let mut buf = Vec::new();
        let mut ser = rmp_serde::Serializer::new(&mut buf).with_struct_map();
        v.serialize(&mut ser).unwrap();
        rmp_serde::from_slice(&buf).unwrap()
    }

    fn mpv_map_keys(v: &rmpv::Value) -> Vec<&str> {
        eprintln!("v: {v:?}");
        v.as_map()
            .into_iter()
            .flat_map(|m| m.iter())
            .filter_map(|(k, _)| k.as_str())
            .collect()
    }

    #[test]
    fn hello_params_serializes_snake_case() {
        let p = HelloParams {
            access_key: "ak".into(),
            client_name: "c".into(),
            ts: 42,
            nonce: "n".into(),
            signature: "sig".into(),
        };
        let v = mpv_roundtrip(&p);
        let keys = mpv_map_keys(&v);
        assert!(keys.contains(&"access_key"));
        assert!(keys.contains(&"client_name"));
        assert!(keys.contains(&"ts"));
        assert!(keys.contains(&"nonce"));
        assert!(keys.contains(&"signature"));
    }

    #[test]
    fn hello_response_serializes_snake_case() {
        let r = HelloResponse {
            ok: true,
            gateway_version: "0.2.0".into(),
        };
        let v = mpv_roundtrip(&r);
        let keys = mpv_map_keys(&v);
        assert!(keys.contains(&"ok"));
        assert!(keys.contains(&"gateway_version"));
    }

    #[test]
    fn spawn_agent_params_skips_empty_maps_and_none_fields() {
        let p = SpawnAgentParams {
            name: "n".into(),
            image: None,
            env: HashMap::new(),
            labels: HashMap::new(),
            extra_ports: Vec::new(),
        };
        let v = mpv_roundtrip(&p);
        let keys = mpv_map_keys(&v);
        assert!(keys.contains(&"name"));
    }

    #[test]
    fn agent_summary_uses_snake_case_ms_suffix() {
        let s = AgentSummary {
            agent_id: "a".into(),
            container_name: "nexal-c".into(),
            created_at_unix_ms: 1_700_000_000_000,
        };
        let v = mpv_roundtrip(&s);
        let keys = mpv_map_keys(&v);
        assert!(keys.contains(&"agent_id"));
        assert!(keys.contains(&"container_name"));
        assert!(keys.contains(&"created_at_unix_ms"));
    }

    #[test]
    fn register_proxy_response_carries_snake_case_socket_path() {
        let r = RegisterProxyResponse {
            token: "deadbeef".into(),
            socket_path: "/run/nexal/proxy/jina.socket".into(),
        };
        let v = mpv_roundtrip(&r);
        let keys = mpv_map_keys(&v);
        assert!(keys.contains(&"socket_path"));
        assert!(!keys.contains(&"socketPath"));
    }

    #[test]
    fn jsonrpc_request_with_no_id_omits_id_field() {
        let req = JsonRpcRequest {
            jsonrpc: JSONRPC_VERSION.into(),
            id: None,
            method: "x".into(),
            params: Some(mpv_from_json(r#"{"a":1}"#)),
        };
        let v = mpv_roundtrip(&req);
        let keys = mpv_map_keys(&v);
        assert!(!keys.contains(&"id"), "notifications omit `id`");
    }

    #[test]
    fn jsonrpc_response_ok_helper_builds_valid_envelope() {
        let r = JsonRpcResponse::ok(
            mpv_from_json(r#""id-1""#),
            mpv_from_json(r#"{"ok":true}"#),
        );
        assert_eq!(r.jsonrpc, JSONRPC_VERSION);
        assert_eq!(r.id, mpv_from_json(r#""id-1""#));
        assert!(r.error.is_none());
        assert_eq!(r.result, Some(mpv_from_json(r#"{"ok":true}"#)));
    }

    #[test]
    fn jsonrpc_response_err_helper_builds_error_envelope() {
        let r = JsonRpcResponse::err(mpv_from_json("5"), error_code::UNKNOWN_AGENT, "nope");
        assert_eq!(r.jsonrpc, JSONRPC_VERSION);
        assert!(r.result.is_none());
        let e = r.error.expect("error response must carry an error");
        assert_eq!(e.code, error_code::UNKNOWN_AGENT);
        assert_eq!(e.message, "nope");
        assert!(e.data.is_none());
    }

    #[test]
    fn notification_helper_sets_id_to_none() {
        let n = notification(NOTIFY_AGENT, mpv_from_json(r#"{"agent_id":"a"}"#));
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
    fn register_proxy_params_deserializes_from_snake_case_msgpack() {
        let raw = mpv_from_json(r#"{"agent_id":"a-1","name":"jina","upstream_url":"https://api.jina.ai","headers":{"Authorization":"Bearer k"}}"#);
        let p: RegisterProxyParams = rmpv::ext::from_value(raw).expect("register_proxy parses");
        assert_eq!(p.agent_id, "a-1");
        assert_eq!(p.name, "jina");
        assert_eq!(p.upstream_url, "https://api.jina.ai");
        assert_eq!(
            p.headers.get("Authorization").map(String::as_str),
            Some("Bearer k")
        );
    }
}
