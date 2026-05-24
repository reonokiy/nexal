//! Typed agent client — RPC against an agent directly (peer is the
//! agent's connection).

use super::methods::{
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
use super::notifications::{
    PROCESS_CLOSED, PROCESS_EXITED, PROCESS_OUTPUT, ProcessClosedNotification,
    ProcessExitedNotification, ProcessOutputNotification,
};
use crate::connection::{Connection, TypedRequestError};

type TypedResult<T> = Result<T, TypedRequestError>;

/// Typed client that talks directly to an agent. Mirrors
/// `createAgentClient` on the TS side.
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
