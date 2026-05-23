use std::sync::Arc;
use tokio::sync::mpsc;

use crate::process::events::{ProcessEvent, ProcessEventBroadcaster};
use crate::protocol::channel::{ChannelNotificationSender, ChannelOutboundMessage};
use crate::protocol::errors::{ChannelError, ChannelErrorKind};
use crate::protocol::wire::{
    ExecParams, ExecResponse, FsCopyParams, FsCopyResponse, FsCreateDirectoryParams,
    FsCreateDirectoryResponse, FsGetMetadataParams, FsGetMetadataResponse, FsReadDirectoryParams,
    FsReadDirectoryResponse, FsReadFileParams, FsReadFileResponse, FsRemoveParams,
    FsRemoveResponse, FsWriteFileParams, FsWriteFileResponse, InitializeResponse,
    ProxyRegisterParams, ProxyRegisterResponse, ProxyUnregisterParams, ProxyUnregisterResponse,
    ReadParams, ReadResponse, TerminateParams, TerminateResponse, WriteParams, WriteResponse,
};
use crate::proxy::ProxyManager;
use crate::server::channel::dispatch::MsgpackChannel;
use crate::server::services::file_system::FileSystemHandler;
use crate::server::services::process::ProcessHandler;

#[derive(Clone)]
pub(crate) struct ExecServerHandler {
    process: ProcessHandler,
    file_system: FileSystemHandler,
    proxy: std::sync::Arc<ProxyManager>,
    channel: Arc<tokio::sync::OnceCell<Arc<MsgpackChannel>>>,
}

impl ExecServerHandler {
    pub(crate) fn new() -> Self {
        let process_events = ProcessEventBroadcaster::new();
        let notifications = discard_notification_sender();
        Self {
            process: ProcessHandler::new(notifications, process_events),
            file_system: FileSystemHandler::default(),
            proxy: std::sync::Arc::new(ProxyManager::new()),
            channel: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    /// Set the bidirectional channel handle. Called once during
    /// dispatch initialization.
    pub(crate) fn set_channel(&self, channel: Arc<MsgpackChannel>) {
        let _ = self.channel.set(channel);
    }

    pub(crate) async fn shutdown(&self) {
        self.proxy.shutdown().await;
        self.process.shutdown().await;
    }

    pub(crate) fn initialize(&self) -> Result<InitializeResponse, ChannelError> {
        self.process.initialize()
    }

    pub(crate) fn initialized(&self) -> Result<(), String> {
        self.process.initialized()?;

        // Mount skills FUSE filesystem if the channel is available.
        if let Some(channel) = self.channel.get().cloned() {
            let mountpoint = "/workspace/agents/skills".to_string();
            tokio::spawn(async move {
                // Create mountpoint directory.
                if let Err(e) = tokio::fs::create_dir_all(&mountpoint).await {
                    tracing::warn!("failed to create skills mountpoint {mountpoint}: {e}");
                    return;
                }
                if let Err(e) = crate::fs::skills::SkillsFuseFs::mount(channel, &mountpoint).await {
                    tracing::warn!("failed to mount skills FUSE at {mountpoint}: {e}");
                }
            });
        }

        Ok(())
    }

    pub(crate) async fn exec(&self, params: ExecParams) -> Result<ExecResponse, ChannelError> {
        self.process.exec(params).await
    }

    pub(crate) async fn exec_read(&self, params: ReadParams) -> Result<ReadResponse, ChannelError> {
        self.process.exec_read(params).await
    }

    pub(crate) async fn exec_write(
        &self,
        params: WriteParams,
    ) -> Result<WriteResponse, ChannelError> {
        self.process.exec_write(params).await
    }

    pub(crate) async fn terminate(
        &self,
        params: TerminateParams,
    ) -> Result<TerminateResponse, ChannelError> {
        self.process.terminate(params).await
    }

    pub(crate) async fn fs_read_file(
        &self,
        params: FsReadFileParams,
    ) -> Result<FsReadFileResponse, ChannelError> {
        self.process.require_initialized_for("filesystem")?;
        self.file_system.read_file(params).await
    }

    pub(crate) async fn fs_write_file(
        &self,
        params: FsWriteFileParams,
    ) -> Result<FsWriteFileResponse, ChannelError> {
        self.process.require_initialized_for("filesystem")?;
        self.file_system.write_file(params).await
    }

    pub(crate) async fn fs_create_directory(
        &self,
        params: FsCreateDirectoryParams,
    ) -> Result<FsCreateDirectoryResponse, ChannelError> {
        self.process.require_initialized_for("filesystem")?;
        self.file_system.create_directory(params).await
    }

    pub(crate) async fn fs_get_metadata(
        &self,
        params: FsGetMetadataParams,
    ) -> Result<FsGetMetadataResponse, ChannelError> {
        self.process.require_initialized_for("filesystem")?;
        self.file_system.get_metadata(params).await
    }

    pub(crate) async fn fs_read_directory(
        &self,
        params: FsReadDirectoryParams,
    ) -> Result<FsReadDirectoryResponse, ChannelError> {
        self.process.require_initialized_for("filesystem")?;
        self.file_system.read_directory(params).await
    }

    pub(crate) async fn fs_remove(
        &self,
        params: FsRemoveParams,
    ) -> Result<FsRemoveResponse, ChannelError> {
        self.process.require_initialized_for("filesystem")?;
        self.file_system.remove(params).await
    }

    pub(crate) async fn fs_copy(
        &self,
        params: FsCopyParams,
    ) -> Result<FsCopyResponse, ChannelError> {
        self.process.require_initialized_for("filesystem")?;
        self.file_system.copy(params).await
    }

    pub(crate) async fn proxy_register(
        &self,
        params: ProxyRegisterParams,
    ) -> Result<ProxyRegisterResponse, ChannelError> {
        self.process.require_initialized_for("proxy")?;
        self.proxy
            .register(&params.socket_path, &params.upstream_url, params.headers)
            .await
            .map_err(|e| ChannelError {
                kind: ChannelErrorKind::Internal,
                message: e,
                data: None,
            })?;
        Ok(ProxyRegisterResponse { ok: true })
    }

    pub(crate) async fn proxy_unregister(
        &self,
        params: ProxyUnregisterParams,
    ) -> Result<ProxyUnregisterResponse, ChannelError> {
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

fn discard_notification_sender() -> ChannelNotificationSender {
    let (outgoing_tx, mut outgoing_rx) = mpsc::channel::<ChannelOutboundMessage>(256);
    tokio::spawn(async move { while outgoing_rx.recv().await.is_some() {} });
    ChannelNotificationSender::new(outgoing_tx)
}
