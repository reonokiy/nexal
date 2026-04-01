use nexal_app_server_protocol::FsCopyParams;
use nexal_app_server_protocol::FsCopyResponse;
use nexal_app_server_protocol::FsCreateDirectoryParams;
use nexal_app_server_protocol::FsCreateDirectoryResponse;
use nexal_app_server_protocol::FsGetMetadataParams;
use nexal_app_server_protocol::FsGetMetadataResponse;
use nexal_app_server_protocol::FsReadDirectoryParams;
use nexal_app_server_protocol::FsReadDirectoryResponse;
use nexal_app_server_protocol::FsReadFileParams;
use nexal_app_server_protocol::FsReadFileResponse;
use nexal_app_server_protocol::FsRemoveParams;
use nexal_app_server_protocol::FsRemoveResponse;
use nexal_app_server_protocol::FsWriteFileParams;
use nexal_app_server_protocol::FsWriteFileResponse;
use nexal_app_server_protocol::JSONRPCErrorError;

use crate::protocol::ExecParams;
use crate::protocol::ExecResponse;
use crate::protocol::InitializeResponse;
use crate::protocol::ReadParams;
use crate::protocol::ReadResponse;
use crate::protocol::TerminateParams;
use crate::protocol::TerminateResponse;
use crate::protocol::WriteParams;
use crate::protocol::WriteResponse;
use crate::protocol::ProxyRegisterParams;
use crate::protocol::ProxyRegisterResponse;
use crate::protocol::ProxyUnregisterParams;
use crate::protocol::ProxyUnregisterResponse;
use crate::proxy::ProxyManager;
use crate::rpc::RpcNotificationSender;
use crate::server::file_system_handler::FileSystemHandler;
use crate::server::process_handler::ProcessHandler;

#[derive(Clone)]
pub(crate) struct ExecServerHandler {
    process: ProcessHandler,
    file_system: FileSystemHandler,
    proxy: std::sync::Arc<ProxyManager>,
}

impl ExecServerHandler {
    pub(crate) fn new(notifications: RpcNotificationSender) -> Self {
        Self {
            process: ProcessHandler::new(notifications),
            file_system: FileSystemHandler::default(),
            proxy: std::sync::Arc::new(ProxyManager::new()),
        }
    }

    pub(crate) async fn shutdown(&self) {
        self.proxy.shutdown().await;
        self.process.shutdown().await;
    }

    pub(crate) fn initialize(&self) -> Result<InitializeResponse, JSONRPCErrorError> {
        self.process.initialize()
    }

    pub(crate) fn initialized(&self) -> Result<(), String> {
        self.process.initialized()
    }

    pub(crate) async fn exec(&self, params: ExecParams) -> Result<ExecResponse, JSONRPCErrorError> {
        self.process.exec(params).await
    }

    pub(crate) async fn exec_read(
        &self,
        params: ReadParams,
    ) -> Result<ReadResponse, JSONRPCErrorError> {
        self.process.exec_read(params).await
    }

    pub(crate) async fn exec_write(
        &self,
        params: WriteParams,
    ) -> Result<WriteResponse, JSONRPCErrorError> {
        self.process.exec_write(params).await
    }

    pub(crate) async fn terminate(
        &self,
        params: TerminateParams,
    ) -> Result<TerminateResponse, JSONRPCErrorError> {
        self.process.terminate(params).await
    }

    pub(crate) async fn fs_read_file(
        &self,
        params: FsReadFileParams,
    ) -> Result<FsReadFileResponse, JSONRPCErrorError> {
        self.process.require_initialized_for("filesystem")?;
        self.file_system.read_file(params).await
    }

    pub(crate) async fn fs_write_file(
        &self,
        params: FsWriteFileParams,
    ) -> Result<FsWriteFileResponse, JSONRPCErrorError> {
        self.process.require_initialized_for("filesystem")?;
        self.file_system.write_file(params).await
    }

    pub(crate) async fn fs_create_directory(
        &self,
        params: FsCreateDirectoryParams,
    ) -> Result<FsCreateDirectoryResponse, JSONRPCErrorError> {
        self.process.require_initialized_for("filesystem")?;
        self.file_system.create_directory(params).await
    }

    pub(crate) async fn fs_get_metadata(
        &self,
        params: FsGetMetadataParams,
    ) -> Result<FsGetMetadataResponse, JSONRPCErrorError> {
        self.process.require_initialized_for("filesystem")?;
        self.file_system.get_metadata(params).await
    }

    pub(crate) async fn fs_read_directory(
        &self,
        params: FsReadDirectoryParams,
    ) -> Result<FsReadDirectoryResponse, JSONRPCErrorError> {
        self.process.require_initialized_for("filesystem")?;
        self.file_system.read_directory(params).await
    }

    pub(crate) async fn fs_remove(
        &self,
        params: FsRemoveParams,
    ) -> Result<FsRemoveResponse, JSONRPCErrorError> {
        self.process.require_initialized_for("filesystem")?;
        self.file_system.remove(params).await
    }

    pub(crate) async fn fs_copy(
        &self,
        params: FsCopyParams,
    ) -> Result<FsCopyResponse, JSONRPCErrorError> {
        self.process.require_initialized_for("filesystem")?;
        self.file_system.copy(params).await
    }

    pub(crate) async fn proxy_register(
        &self,
        params: ProxyRegisterParams,
    ) -> Result<ProxyRegisterResponse, JSONRPCErrorError> {
        self.process.require_initialized_for("proxy")?;
        self.proxy
            .register(&params.socket_path, &params.upstream_url, params.headers)
            .await
            .map_err(|e| JSONRPCErrorError {
                code: -32603,
                message: e,
                data: None,
            })?;
        Ok(ProxyRegisterResponse { ok: true })
    }

    pub(crate) async fn proxy_unregister(
        &self,
        params: ProxyUnregisterParams,
    ) -> Result<ProxyUnregisterResponse, JSONRPCErrorError> {
        self.process.require_initialized_for("proxy")?;
        let ok = self.proxy.unregister(&params.socket_path).await;
        Ok(ProxyUnregisterResponse { ok })
    }
}

#[cfg(test)]
mod tests;
