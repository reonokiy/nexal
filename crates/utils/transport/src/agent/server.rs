//! Server-side trait + installer for the Agent method set.

use std::sync::Arc;

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
use crate::connection::Connection;
use crate::{WireError, method_not_found};

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
