use tokio::sync::mpsc;

use crate::transport::protocol::FsCopyParams;
use crate::transport::protocol::FsCopyResponse;
use crate::transport::protocol::FsCreateDirectoryParams;
use crate::transport::protocol::FsCreateDirectoryResponse;
use crate::transport::protocol::FsGetMetadataParams;
use crate::transport::protocol::FsGetMetadataResponse;
use crate::transport::protocol::FsReadDirectoryParams;
use crate::transport::protocol::FsReadDirectoryResponse;
use crate::transport::protocol::FsReadFileParams;
use crate::transport::protocol::FsReadFileResponse;
use crate::transport::protocol::FsRemoveParams;
use crate::transport::protocol::FsRemoveResponse;
use crate::transport::protocol::FsWriteFileParams;
use crate::transport::protocol::FsWriteFileResponse;
use crate::transport::protocol::JSONRPCErrorError;

use crate::transport::protocol::ExecParams;
use crate::transport::protocol::ExecResponse;
use crate::transport::protocol::InitializeResponse;
use crate::transport::protocol::ProxyRegisterParams;
use crate::transport::protocol::ProxyRegisterResponse;
use crate::transport::protocol::ProxyUnregisterParams;
use crate::transport::protocol::ProxyUnregisterResponse;
use crate::transport::protocol::ReadParams;
use crate::transport::protocol::ReadResponse;
use crate::transport::protocol::TerminateParams;
use crate::transport::protocol::TerminateResponse;
use crate::transport::protocol::WriteParams;
use crate::transport::protocol::WriteResponse;
use crate::proxy::ProxyManager;
use crate::transport::rpc::RpcNotificationSender;
use crate::transport::rpc::RpcServerOutboundMessage;
use crate::server::services::file_system::FileSystemHandler;
use crate::server::services::process::ProcessHandler;
use crate::server::services::{ProcessEvent, ProcessEventBroadcaster};

#[derive(Clone)]
pub(crate) struct ExecServerHandler {
    process: ProcessHandler,
    file_system: FileSystemHandler,
    proxy: std::sync::Arc<ProxyManager>,
}

impl ExecServerHandler {
    pub(crate) fn new() -> Self {
        let process_events = ProcessEventBroadcaster::new();
        let notifications = discard_notification_sender();
        Self {
            process: ProcessHandler::new(notifications, process_events),
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

    pub(crate) fn subscribe_process_events(
        &self,
    ) -> tokio::sync::broadcast::Receiver<ProcessEvent> {
        self.process.subscribe_events()
    }
}

fn discard_notification_sender() -> RpcNotificationSender {
    let (outgoing_tx, mut outgoing_rx) = mpsc::channel::<RpcServerOutboundMessage>(256);
    tokio::spawn(async move { while outgoing_rx.recv().await.is_some() {} });
    RpcNotificationSender::new(outgoing_tx)
}
