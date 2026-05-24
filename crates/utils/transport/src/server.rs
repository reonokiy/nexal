//! Typed RPC servers — Rust trait-based mirror of TS `server.ts`.
//!
//! Instead of a TS-style "struct of optional callbacks" builder, the
//! Rust API exposes a trait per method-set ([`AgentHandlers`],
//! [`GatewayHandlers`]) where each method has a default implementation
//! that returns `MethodNotFound`. Users implement only the methods
//! they care about and then call [`serve_agent`] / [`serve_gateway`]
//! to install everything on a [`Connection`].
//!
//! # Example
//!
//! ```no_run
//! # use std::sync::Arc;
//! # use nexal_utils_transport::agent::{InitializeParams, InitializeResponse};
//! # use nexal_utils_transport::connection::Connection;
//! # use nexal_utils_transport::server::{AgentHandlers, serve_agent};
//! # use nexal_utils_transport::WireError;
//! struct MyAgent;
//!
//! impl AgentHandlers for MyAgent {
//!     async fn initialize(&self, _p: InitializeParams) -> Result<InitializeResponse, WireError> {
//!         Ok(InitializeResponse { default_shell: Some("/bin/sh".into()), cwd: None })
//!     }
//! }
//!
//! # async fn demo(conn: Connection) {
//! serve_agent(&conn, Arc::new(MyAgent)).await;
//! # }
//! ```

use std::sync::Arc;

use rmpv::Value;

use crate::WireError;
use crate::agent::{
    AgentFsCopy, AgentFsCreateDirectory, AgentFsGetMetadata, AgentFsReadDirectory,
    AgentFsReadFile, AgentFsRemove, AgentFsWriteFile, AgentInitialize, AgentInitialized,
    AgentProcessRead, AgentProcessStart, AgentProcessTerminate, AgentProcessWrite,
    AgentProxyRegister, AgentProxyUnregister, FsCopyParams, FsCopyResponse,
    FsCreateDirectoryParams, FsCreateDirectoryResponse, FsGetMetadataParams,
    FsGetMetadataResponse, FsReadDirectoryParams, FsReadDirectoryResponse, FsReadFileParams,
    FsReadFileResponse, FsRemoveParams, FsRemoveResponse, FsWriteFileParams,
    FsWriteFileResponse, InitializeParams, InitializeResponse, ProcessReadParams,
    ProcessReadResponse, ProcessStartParams, ProcessStartResponse, ProcessTerminateParams,
    ProcessTerminateResponse, ProcessWriteParams, ProcessWriteResponse, ProxyRegisterParams,
    ProxyRegisterResponse, ProxyUnregisterParams, ProxyUnregisterResponse,
};
use crate::connection::Connection;
use crate::gateway::{
    AgentIdParams, AgentInvokeParams, AttachAgentParams, GatewayAgentInvoke, GatewayAttachAgent,
    GatewayDetachAgent, GatewayHello, GatewayKillAgent, GatewayListAgents, GatewayRegisterProxy,
    GatewayRegisterStreamProxy, GatewaySpawnAgent, GatewayUnregisterProxy,
    GatewayUnregisterStreamProxy, HelloParams, HelloResponse, ListAgentsResponse, OkResponse,
    RegisterProxyParams, RegisterProxyResponse, RegisterStreamProxyParams,
    RegisterStreamProxyResponse, SpawnAgentParams, SpawnAgentResponse, UnregisterProxyParams,
    UnregisterStreamProxyParams,
};

/// Construct a JSON-RPC style "method not found" [`WireError`].
pub fn method_not_found(method: &str) -> WireError {
    WireError {
        code: -32601,
        message: format!("method not found: {method}"),
        data: None,
    }
}

// ── AgentHandlers ───────────────────────────────────────────────────

/// Server-side trait for the Agent method set.
///
/// Every method has a default implementation returning
/// [`method_not_found`]. Implementors override only the methods they
/// support.
pub trait AgentHandlers: Send + Sync + 'static {
    fn initialize(
        &self,
        _p: InitializeParams,
    ) -> impl std::future::Future<Output = Result<InitializeResponse, WireError>> + Send {
        async { Err(method_not_found("initialize")) }
    }

    fn initialized(
        &self,
    ) -> impl std::future::Future<Output = Result<(), WireError>> + Send {
        async { Err(method_not_found("initialized")) }
    }

    fn process_start(
        &self,
        _p: ProcessStartParams,
    ) -> impl std::future::Future<Output = Result<ProcessStartResponse, WireError>> + Send {
        async { Err(method_not_found("process/start")) }
    }

    fn process_read(
        &self,
        _p: ProcessReadParams,
    ) -> impl std::future::Future<Output = Result<ProcessReadResponse, WireError>> + Send {
        async { Err(method_not_found("process/read")) }
    }

    fn process_write(
        &self,
        _p: ProcessWriteParams,
    ) -> impl std::future::Future<Output = Result<ProcessWriteResponse, WireError>> + Send {
        async { Err(method_not_found("process/write")) }
    }

    fn process_terminate(
        &self,
        _p: ProcessTerminateParams,
    ) -> impl std::future::Future<Output = Result<ProcessTerminateResponse, WireError>> + Send
    {
        async { Err(method_not_found("process/terminate")) }
    }

    fn fs_read_file(
        &self,
        _p: FsReadFileParams,
    ) -> impl std::future::Future<Output = Result<FsReadFileResponse, WireError>> + Send {
        async { Err(method_not_found("fs/read_file")) }
    }

    fn fs_write_file(
        &self,
        _p: FsWriteFileParams,
    ) -> impl std::future::Future<Output = Result<FsWriteFileResponse, WireError>> + Send {
        async { Err(method_not_found("fs/write_file")) }
    }

    fn fs_create_directory(
        &self,
        _p: FsCreateDirectoryParams,
    ) -> impl std::future::Future<Output = Result<FsCreateDirectoryResponse, WireError>> + Send
    {
        async { Err(method_not_found("fs/create_directory")) }
    }

    fn fs_get_metadata(
        &self,
        _p: FsGetMetadataParams,
    ) -> impl std::future::Future<Output = Result<FsGetMetadataResponse, WireError>> + Send {
        async { Err(method_not_found("fs/get_metadata")) }
    }

    fn fs_read_directory(
        &self,
        _p: FsReadDirectoryParams,
    ) -> impl std::future::Future<Output = Result<FsReadDirectoryResponse, WireError>> + Send
    {
        async { Err(method_not_found("fs/read_directory")) }
    }

    fn fs_remove(
        &self,
        _p: FsRemoveParams,
    ) -> impl std::future::Future<Output = Result<FsRemoveResponse, WireError>> + Send {
        async { Err(method_not_found("fs/remove")) }
    }

    fn fs_copy(
        &self,
        _p: FsCopyParams,
    ) -> impl std::future::Future<Output = Result<FsCopyResponse, WireError>> + Send {
        async { Err(method_not_found("fs/copy")) }
    }

    fn proxy_register(
        &self,
        _p: ProxyRegisterParams,
    ) -> impl std::future::Future<Output = Result<ProxyRegisterResponse, WireError>> + Send {
        async { Err(method_not_found("proxy/register")) }
    }

    fn proxy_unregister(
        &self,
        _p: ProxyUnregisterParams,
    ) -> impl std::future::Future<Output = Result<ProxyUnregisterResponse, WireError>> + Send
    {
        async { Err(method_not_found("proxy/unregister")) }
    }
}

/// Install every method of an [`AgentHandlers`] implementation on `conn`.
pub async fn serve_agent<H: AgentHandlers>(conn: &Connection, handlers: Arc<H>) {
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

    bind!(AgentInitialize, initialize);
    bind!(AgentInitialized, initialized, no_params);
    bind!(AgentProcessStart, process_start);
    bind!(AgentProcessRead, process_read);
    bind!(AgentProcessWrite, process_write);
    bind!(AgentProcessTerminate, process_terminate);
    bind!(AgentFsReadFile, fs_read_file);
    bind!(AgentFsWriteFile, fs_write_file);
    bind!(AgentFsCreateDirectory, fs_create_directory);
    bind!(AgentFsGetMetadata, fs_get_metadata);
    bind!(AgentFsReadDirectory, fs_read_directory);
    bind!(AgentFsRemove, fs_remove);
    bind!(AgentFsCopy, fs_copy);
    bind!(AgentProxyRegister, proxy_register);
    bind!(AgentProxyUnregister, proxy_unregister);
}

// ── GatewayHandlers ─────────────────────────────────────────────────

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
