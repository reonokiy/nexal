use crate::process::events::{ProcessEvent, ProcessEventBroadcaster};
use crate::process::local::LocalProcess;
use crate::protocol::channel::ChannelNotificationSender;
use crate::protocol::errors::ChannelError;
use crate::protocol::wire::{
    ExecParams, ExecResponse, InitializeResponse, ReadParams, ReadResponse, TerminateParams,
    TerminateResponse, WriteParams, WriteResponse,
};

#[derive(Clone)]
pub(crate) struct ProcessHandler {
    process: LocalProcess,
}

impl ProcessHandler {
    pub(crate) fn new(
        notifications: ChannelNotificationSender,
        process_events: ProcessEventBroadcaster,
    ) -> Self {
        Self {
            process: LocalProcess::new(notifications, process_events),
        }
    }

    pub(crate) async fn shutdown(&self) {
        self.process.shutdown().await;
    }

    pub(crate) fn initialize(&self) -> Result<InitializeResponse, ChannelError> {
        self.process.initialize()
    }

    pub(crate) fn initialized(&self) -> Result<(), String> {
        self.process.initialized()
    }

    pub(crate) fn require_initialized_for(&self, method_family: &str) -> Result<(), ChannelError> {
        self.process.require_initialized_for(method_family)
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
        self.process.terminate_process(params).await
    }

    pub(crate) fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<ProcessEvent> {
        self.process.subscribe_events()
    }
}
