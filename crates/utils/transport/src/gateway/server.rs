//! Server-side trait + installer for the Gateway method set.

use std::sync::Arc;

use rmpv::Value;

use super::methods::{
    AgentIdParams, AgentInvokeParams, AttachAgentParams, GatewayAgentInvoke, GatewayAttachAgent,
    GatewayDetachAgent, GatewayHello, GatewayKillAgent, GatewayListAgents, GatewayRegisterProxy,
    GatewayRegisterStreamProxy, GatewaySpawnAgent, GatewayUnregisterProxy,
    GatewayUnregisterStreamProxy, HelloParams, HelloResponse, ListAgentsResponse, OkResponse,
    RegisterProxyParams, RegisterProxyResponse, RegisterStreamProxyParams,
    RegisterStreamProxyResponse, SpawnAgentParams, SpawnAgentResponse, UnregisterProxyParams,
    UnregisterStreamProxyParams,
};
use crate::connection::Connection;
use crate::{WireError, method_not_found};

/// Server-side trait for the Gateway method set.
pub trait GatewayHandlers: Send + Sync + 'static {
    fn hello(
        &self,
        _p: HelloParams,
    ) -> impl std::future::Future<Output = Result<HelloResponse, WireError>> + Send {
        async { Err(method_not_found("gateway/hello")) }
    }

    fn spawn_agent(
        &self,
        _p: SpawnAgentParams,
    ) -> impl std::future::Future<Output = Result<SpawnAgentResponse, WireError>> + Send {
        async { Err(method_not_found("gateway/spawn_agent")) }
    }

    fn kill_agent(
        &self,
        _p: AgentIdParams,
    ) -> impl std::future::Future<Output = Result<OkResponse, WireError>> + Send {
        async { Err(method_not_found("gateway/kill_agent")) }
    }

    fn detach_agent(
        &self,
        _p: AgentIdParams,
    ) -> impl std::future::Future<Output = Result<OkResponse, WireError>> + Send {
        async { Err(method_not_found("gateway/detach_agent")) }
    }

    fn attach_agent(
        &self,
        _p: AttachAgentParams,
    ) -> impl std::future::Future<Output = Result<SpawnAgentResponse, WireError>> + Send {
        async { Err(method_not_found("gateway/attach_agent")) }
    }

    fn list_agents(
        &self,
    ) -> impl std::future::Future<Output = Result<ListAgentsResponse, WireError>> + Send {
        async { Err(method_not_found("gateway/list_agents")) }
    }

    fn register_proxy(
        &self,
        _p: RegisterProxyParams,
    ) -> impl std::future::Future<Output = Result<RegisterProxyResponse, WireError>> + Send {
        async { Err(method_not_found("gateway/register_proxy")) }
    }

    fn unregister_proxy(
        &self,
        _p: UnregisterProxyParams,
    ) -> impl std::future::Future<Output = Result<OkResponse, WireError>> + Send {
        async { Err(method_not_found("gateway/unregister_proxy")) }
    }

    fn register_stream_proxy(
        &self,
        _p: RegisterStreamProxyParams,
    ) -> impl std::future::Future<Output = Result<RegisterStreamProxyResponse, WireError>> + Send
    {
        async { Err(method_not_found("gateway/register_stream_proxy")) }
    }

    fn unregister_stream_proxy(
        &self,
        _p: UnregisterStreamProxyParams,
    ) -> impl std::future::Future<Output = Result<OkResponse, WireError>> + Send {
        async { Err(method_not_found("gateway/unregister_stream_proxy")) }
    }

    fn agent_invoke(
        &self,
        _p: AgentInvokeParams,
    ) -> impl std::future::Future<Output = Result<Value, WireError>> + Send {
        async { Err(method_not_found("agent/invoke")) }
    }
}

/// Install every method of a [`GatewayHandlers`] implementation on `conn`.
pub async fn serve_gateway<H: GatewayHandlers>(conn: &Connection, handlers: Arc<H>) {
    macro_rules! bind {
        ($M:ty, $method:ident) => {{
            let h = handlers.clone();
            conn.handle_request_typed::<$M, _, _>(move |p| {
                let h = h.clone();
                async move { h.$method(p).await }
            })
            .await;
        }};
        ($M:ty, $method:ident, no_params) => {{
            let h = handlers.clone();
            conn.handle_request_typed::<$M, _, _>(move |_p: ()| {
                let h = h.clone();
                async move { h.$method().await }
            })
            .await;
        }};
    }

    bind!(GatewayHello, hello);
    bind!(GatewaySpawnAgent, spawn_agent);
    bind!(GatewayKillAgent, kill_agent);
    bind!(GatewayDetachAgent, detach_agent);
    bind!(GatewayAttachAgent, attach_agent);
    bind!(GatewayListAgents, list_agents, no_params);
    bind!(GatewayRegisterProxy, register_proxy);
    bind!(GatewayUnregisterProxy, unregister_proxy);
    bind!(GatewayRegisterStreamProxy, register_stream_proxy);
    bind!(GatewayUnregisterStreamProxy, unregister_stream_proxy);
    bind!(GatewayAgentInvoke, agent_invoke);
}
