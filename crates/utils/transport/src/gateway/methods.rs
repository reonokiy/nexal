use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Notification method emitted by the gateway whenever a proxied agent
/// sends a notification its way.
pub const NOTIFY_AGENT: &str = "agent/notify";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HelloParams {
    pub access_key: String,
    pub client_name: String,
    pub ts: i64,
    pub nonce: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HelloResponse {
    pub ok: bool,
    pub gateway_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SpawnAgentParams {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub labels: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_ports: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SpawnAgentResponse {
    pub agent_id: String,
    pub container_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentIdParams {
    pub agent_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OkResponse {
    pub ok: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AttachAgentParams {
    pub container_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentSummary {
    pub agent_id: String,
    pub container_name: String,
    pub created_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ListAgentsResponse {
    pub agents: Vec<AgentSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentInvokeParams {
    pub agent_id: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<rmpv::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentNotifyParams {
    pub agent_id: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<rmpv::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RegisterProxyParams {
    pub agent_id: String,
    pub name: String,
    pub upstream_url: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RegisterProxyResponse {
    pub token: String,
    pub socket_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UnregisterProxyParams {
    pub agent_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RegisterStreamProxyParams {
    pub agent_id: String,
    pub name: String,
    pub container_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RegisterStreamProxyResponse {
    pub listen_addr: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UnregisterStreamProxyParams {
    pub agent_id: String,
    pub name: String,
}

define_method_set! {
    /// Gateway-side RPC method set.
    enum GatewayMethod {
        Hello                = "gateway/hello"                  => GatewayHello(HelloParams) -> HelloResponse,
        SpawnAgent           = "gateway/spawn_agent"            => GatewaySpawnAgent(SpawnAgentParams) -> SpawnAgentResponse,
        KillAgent            = "gateway/kill_agent"             => GatewayKillAgent(AgentIdParams) -> OkResponse,
        DetachAgent          = "gateway/detach_agent"           => GatewayDetachAgent(AgentIdParams) -> OkResponse,
        AttachAgent          = "gateway/attach_agent"           => GatewayAttachAgent(AttachAgentParams) -> SpawnAgentResponse,
        ListAgents           = "gateway/list_agents"            => GatewayListAgents(()) -> ListAgentsResponse,
        AgentInvoke          = "agent/invoke"                   => GatewayAgentInvoke(AgentInvokeParams) -> rmpv::Value,
        RegisterProxy        = "gateway/register_proxy"         => GatewayRegisterProxy(RegisterProxyParams) -> RegisterProxyResponse,
        UnregisterProxy      = "gateway/unregister_proxy"       => GatewayUnregisterProxy(UnregisterProxyParams) -> OkResponse,
        RegisterStreamProxy  = "gateway/register_stream_proxy"  => GatewayRegisterStreamProxy(RegisterStreamProxyParams) -> RegisterStreamProxyResponse,
        UnregisterStreamProxy = "gateway/unregister_stream_proxy" => GatewayUnregisterStreamProxy(UnregisterStreamProxyParams) -> OkResponse,
    }
}
