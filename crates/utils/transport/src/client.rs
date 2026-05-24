//! Typed RPC clients — Rust mirror of `packages/transport/src/client.ts`.
//!
//! Provides ergonomic wrappers over a [`Connection`] for the Agent,
//! Gateway, and Gateway→Agent (`agent/invoke` proxy) method sets. Each
//! wrapper exposes one async method per RPC, fully typed via the
//! existing `RpcMethod` impls in [`crate::agent`] / [`crate::gateway`].

use rmpv::Value;

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
use crate::connection::{Connection, TypedRequestError};
use crate::gateway::{
    AgentIdParams, AgentInvokeParams, AttachAgentParams, GatewayAgentInvoke, GatewayAttachAgent,
    GatewayDetachAgent, GatewayHello, GatewayKillAgent, GatewayListAgents, GatewayRegisterProxy,
    GatewayRegisterStreamProxy, GatewaySpawnAgent, GatewayUnregisterProxy,
    GatewayUnregisterStreamProxy, HelloParams, HelloResponse, ListAgentsResponse, OkResponse,
    RegisterProxyParams, RegisterProxyResponse, RegisterStreamProxyParams,
    RegisterStreamProxyResponse, SpawnAgentParams, SpawnAgentResponse, UnregisterProxyParams,
    UnregisterStreamProxyParams,
};
use crate::notifications::{
    PROCESS_CLOSED, PROCESS_EXITED, PROCESS_OUTPUT, ProcessClosedNotification,
    ProcessExitedNotification, ProcessOutputNotification,
};
use crate::{RpcMethod, from_msgpack_value, to_msgpack_value};

type TypedResult<T> = Result<T, TypedRequestError>;

// ── AgentClient ─────────────────────────────────────────────────────

/// Typed client that talks directly to an agent (peer is the agent's
/// connection). Mirrors `createAgentClient` in TS.
#[derive(Clone)]
pub struct AgentClient {
    conn: Connection,
}

impl AgentClient {
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub async fn initialize(&self, p: &InitializeParams) -> TypedResult<InitializeResponse> {
        self.conn.request_typed::<AgentInitialize>(p).await
    }

    pub async fn initialized(&self) -> TypedResult<()> {
        self.conn.request_typed::<AgentInitialized>(&()).await
    }

    pub async fn process_start(
        &self,
        p: &ProcessStartParams,
    ) -> TypedResult<ProcessStartResponse> {
        self.conn.request_typed::<AgentProcessStart>(p).await
    }

    pub async fn process_read(&self, p: &ProcessReadParams) -> TypedResult<ProcessReadResponse> {
        self.conn.request_typed::<AgentProcessRead>(p).await
    }

    pub async fn process_write(
        &self,
        p: &ProcessWriteParams,
    ) -> TypedResult<ProcessWriteResponse> {
        self.conn.request_typed::<AgentProcessWrite>(p).await
    }

    pub async fn process_terminate(
        &self,
        p: &ProcessTerminateParams,
    ) -> TypedResult<ProcessTerminateResponse> {
        self.conn.request_typed::<AgentProcessTerminate>(p).await
    }

    pub async fn fs_read_file(&self, p: &FsReadFileParams) -> TypedResult<FsReadFileResponse> {
        self.conn.request_typed::<AgentFsReadFile>(p).await
    }

    pub async fn fs_write_file(
        &self,
        p: &FsWriteFileParams,
    ) -> TypedResult<FsWriteFileResponse> {
        self.conn.request_typed::<AgentFsWriteFile>(p).await
    }

    pub async fn fs_create_directory(
        &self,
        p: &FsCreateDirectoryParams,
    ) -> TypedResult<FsCreateDirectoryResponse> {
        self.conn.request_typed::<AgentFsCreateDirectory>(p).await
    }

    pub async fn fs_get_metadata(
        &self,
        p: &FsGetMetadataParams,
    ) -> TypedResult<FsGetMetadataResponse> {
        self.conn.request_typed::<AgentFsGetMetadata>(p).await
    }

    pub async fn fs_read_directory(
        &self,
        p: &FsReadDirectoryParams,
    ) -> TypedResult<FsReadDirectoryResponse> {
        self.conn.request_typed::<AgentFsReadDirectory>(p).await
    }

    pub async fn fs_remove(&self, p: &FsRemoveParams) -> TypedResult<FsRemoveResponse> {
        self.conn.request_typed::<AgentFsRemove>(p).await
    }

    pub async fn fs_copy(&self, p: &FsCopyParams) -> TypedResult<FsCopyResponse> {
        self.conn.request_typed::<AgentFsCopy>(p).await
    }

    pub async fn proxy_register(
        &self,
        p: &ProxyRegisterParams,
    ) -> TypedResult<ProxyRegisterResponse> {
        self.conn.request_typed::<AgentProxyRegister>(p).await
    }

    pub async fn proxy_unregister(
        &self,
        p: &ProxyUnregisterParams,
    ) -> TypedResult<ProxyUnregisterResponse> {
        self.conn.request_typed::<AgentProxyUnregister>(p).await
    }

    // ── Typed notification subscriptions ────────────────────────────

    pub async fn on_process_output<F>(&self, handler: F)
    where
        F: Fn(ProcessOutputNotification) + Send + Sync + 'static,
    {
        self.conn
            .on_typed::<ProcessOutputNotification, _>(PROCESS_OUTPUT, handler)
            .await;
    }

    pub async fn on_process_exited<F>(&self, handler: F)
    where
        F: Fn(ProcessExitedNotification) + Send + Sync + 'static,
    {
        self.conn
            .on_typed::<ProcessExitedNotification, _>(PROCESS_EXITED, handler)
            .await;
    }

    pub async fn on_process_closed<F>(&self, handler: F)
    where
        F: Fn(ProcessClosedNotification) + Send + Sync + 'static,
    {
        self.conn
            .on_typed::<ProcessClosedNotification, _>(PROCESS_CLOSED, handler)
            .await;
    }
}

// ── GatewayClient ───────────────────────────────────────────────────

/// Typed client that talks to a gateway. Mirrors `createGatewayClient`.
#[derive(Clone)]
pub struct GatewayClient {
    conn: Connection,
}

impl GatewayClient {
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub async fn hello(&self, p: &HelloParams) -> TypedResult<HelloResponse> {
        self.conn.request_typed::<GatewayHello>(p).await
    }

    pub async fn spawn_agent(&self, p: &SpawnAgentParams) -> TypedResult<SpawnAgentResponse> {
        self.conn.request_typed::<GatewaySpawnAgent>(p).await
    }

    pub async fn kill_agent(&self, p: &AgentIdParams) -> TypedResult<OkResponse> {
        self.conn.request_typed::<GatewayKillAgent>(p).await
    }

    pub async fn detach_agent(&self, p: &AgentIdParams) -> TypedResult<OkResponse> {
        self.conn.request_typed::<GatewayDetachAgent>(p).await
    }

    pub async fn attach_agent(&self, p: &AttachAgentParams) -> TypedResult<SpawnAgentResponse> {
        self.conn.request_typed::<GatewayAttachAgent>(p).await
    }

    pub async fn list_agents(&self) -> TypedResult<ListAgentsResponse> {
        self.conn.request_typed::<GatewayListAgents>(&()).await
    }

    pub async fn register_proxy(
        &self,
        p: &RegisterProxyParams,
    ) -> TypedResult<RegisterProxyResponse> {
        self.conn.request_typed::<GatewayRegisterProxy>(p).await
    }

    pub async fn unregister_proxy(
        &self,
        p: &UnregisterProxyParams,
    ) -> TypedResult<OkResponse> {
        self.conn.request_typed::<GatewayUnregisterProxy>(p).await
    }

    pub async fn register_stream_proxy(
        &self,
        p: &RegisterStreamProxyParams,
    ) -> TypedResult<RegisterStreamProxyResponse> {
        self.conn
            .request_typed::<GatewayRegisterStreamProxy>(p)
            .await
    }

    pub async fn unregister_stream_proxy(
        &self,
        p: &UnregisterStreamProxyParams,
    ) -> TypedResult<OkResponse> {
        self.conn
            .request_typed::<GatewayUnregisterStreamProxy>(p)
            .await
    }
}

// ── GatewayAgentClient (proxy via agent/invoke) ─────────────────────

/// Typed client that talks to an agent _through_ a gateway via the
/// `agent/invoke` RPC. Mirrors `createGatewayAgentClient`.
#[derive(Clone)]
pub struct GatewayAgentClient {
    conn: Connection,
    agent_id: String,
}

impl GatewayAgentClient {
    pub fn new(conn: Connection, agent_id: impl Into<String>) -> Self {
        Self {
            conn,
            agent_id: agent_id.into(),
        }
    }

    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    async fn invoke<M>(&self, params: &M::Params) -> TypedResult<M::Result>
    where
        M: RpcMethod,
    {
        let inner = to_msgpack_value(params).map_err(TypedRequestError::EncodeParams)?;
        let invoke_params = AgentInvokeParams {
            agent_id: self.agent_id.clone(),
            method: M::METHOD.into(),
            params: Some(inner),
        };
        let value: Value = self
            .conn
            .request_typed::<GatewayAgentInvoke>(&invoke_params)
            .await?;
        from_msgpack_value::<M::Result>(value)
            .map_err(|e| TypedRequestError::DecodeResult(e.to_string()))
    }

    pub async fn initialize(&self, p: &InitializeParams) -> TypedResult<InitializeResponse> {
        self.invoke::<AgentInitialize>(p).await
    }

    pub async fn initialized(&self) -> TypedResult<()> {
        self.invoke::<AgentInitialized>(&()).await
    }

    pub async fn process_start(
        &self,
        p: &ProcessStartParams,
    ) -> TypedResult<ProcessStartResponse> {
        self.invoke::<AgentProcessStart>(p).await
    }

    pub async fn process_read(&self, p: &ProcessReadParams) -> TypedResult<ProcessReadResponse> {
        self.invoke::<AgentProcessRead>(p).await
    }

    pub async fn process_write(
        &self,
        p: &ProcessWriteParams,
    ) -> TypedResult<ProcessWriteResponse> {
        self.invoke::<AgentProcessWrite>(p).await
    }

    pub async fn process_terminate(
        &self,
        p: &ProcessTerminateParams,
    ) -> TypedResult<ProcessTerminateResponse> {
        self.invoke::<AgentProcessTerminate>(p).await
    }

    pub async fn fs_read_file(&self, p: &FsReadFileParams) -> TypedResult<FsReadFileResponse> {
        self.invoke::<AgentFsReadFile>(p).await
    }

    pub async fn fs_write_file(
        &self,
        p: &FsWriteFileParams,
    ) -> TypedResult<FsWriteFileResponse> {
        self.invoke::<AgentFsWriteFile>(p).await
    }

    pub async fn fs_create_directory(
        &self,
        p: &FsCreateDirectoryParams,
    ) -> TypedResult<FsCreateDirectoryResponse> {
        self.invoke::<AgentFsCreateDirectory>(p).await
    }

    pub async fn fs_get_metadata(
        &self,
        p: &FsGetMetadataParams,
    ) -> TypedResult<FsGetMetadataResponse> {
        self.invoke::<AgentFsGetMetadata>(p).await
    }

    pub async fn fs_read_directory(
        &self,
        p: &FsReadDirectoryParams,
    ) -> TypedResult<FsReadDirectoryResponse> {
        self.invoke::<AgentFsReadDirectory>(p).await
    }

    pub async fn fs_remove(&self, p: &FsRemoveParams) -> TypedResult<FsRemoveResponse> {
        self.invoke::<AgentFsRemove>(p).await
    }

    pub async fn fs_copy(&self, p: &FsCopyParams) -> TypedResult<FsCopyResponse> {
        self.invoke::<AgentFsCopy>(p).await
    }

    pub async fn proxy_register(
        &self,
        p: &ProxyRegisterParams,
    ) -> TypedResult<ProxyRegisterResponse> {
        self.invoke::<AgentProxyRegister>(p).await
    }

    pub async fn proxy_unregister(
        &self,
        p: &ProxyUnregisterParams,
    ) -> TypedResult<ProxyUnregisterResponse> {
        self.invoke::<AgentProxyUnregister>(p).await
    }
}
