use crate::transport::protocol::JSONRPCErrorError;

use crate::executor::local_process::LocalProcess;
use crate::transport::protocol::ExecParams;
use crate::transport::protocol::ExecResponse;
use crate::transport::protocol::InitializeResponse;
use crate::transport::protocol::ReadParams;
use crate::transport::protocol::ReadResponse;
use crate::transport::protocol::TerminateParams;
use crate::transport::protocol::TerminateResponse;
use crate::transport::protocol::WriteParams;
use crate::transport::protocol::WriteResponse;
use crate::transport::rpc::RpcNotificationSender;
use crate::server::services::{ProcessEvent, ProcessEventBroadcaster};

#[derive(Clone)]
pub(crate) struct ProcessHandler {
    process: LocalProcess,
}

impl ProcessHandler {
    pub(crate) fn new(
        notifications: RpcNotificationSender,
        process_events: ProcessEventBroadcaster,
    ) -> Self {
        Self {
            process: LocalProcess::new(notifications, process_events),
        }
    }

    pub(crate) async fn shutdown(&self) {
        self.process.shutdown().await;
    }

    pub(crate) fn initialize(&self) -> Result<InitializeResponse, JSONRPCErrorError> {
        self.process.initialize()
    }

    pub(crate) fn initialized(&self) -> Result<(), String> {
        self.process.initialized()
    }

    pub(crate) fn require_initialized_for(
        &self,
        method_family: &str,
    ) -> Result<(), JSONRPCErrorError> {
        self.process.require_initialized_for(method_family)
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
        self.process.terminate_process(params).await
    }

    pub(crate) fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<ProcessEvent> {
        self.process.subscribe_events()
    }
}
