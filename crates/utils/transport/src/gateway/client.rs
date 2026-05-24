//! Typed gateway clients.
//!
//! Two flavors:
//!   - [`GatewayClient`]      — peer talks to the gateway directly
//!   - [`GatewayAgentClient`] — peer talks to the gateway, which forwards
//!     to an agent via the `agent/invoke` RPC

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
use crate::agent::methods::{
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
use crate::{RpcMethod, from_msgpack_value, to_msgpack_value};

type TypedResult<T> = Result<T, TypedRequestError>;

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
