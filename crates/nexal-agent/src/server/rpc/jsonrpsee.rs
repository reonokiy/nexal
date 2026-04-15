use std::sync::Arc;

use jsonrpsee::core::{SubscriptionResult, async_trait};
use jsonrpsee::server::{PendingSubscriptionSink, RpcModule};
use jsonrpsee::types::ErrorObjectOwned;

use crate::ProcessId;
use crate::protocol::{
    ExecParams, ExecResponse, FsCopyParams, FsCopyResponse, FsCreateDirectoryParams,
    FsCreateDirectoryResponse, FsGetMetadataParams, FsGetMetadataResponse, FsReadDirectoryParams,
    FsReadDirectoryResponse, FsReadFileParams, FsReadFileResponse, FsRemoveParams,
    FsRemoveResponse, FsWriteFileParams, FsWriteFileResponse, InitializeParams, InitializeResponse,
    JSONRPCErrorError, ProxyRegisterParams, ProxyRegisterResponse, ProxyUnregisterParams,
    ProxyUnregisterResponse, ReadParams, ReadResponse, TerminateParams, TerminateResponse,
    WriteParams, WriteResponse,
};
use crate::server::{ExecServerHandler, ProcessEvent};
use crate::transport::jsonrpsee_api::ExecServerJsonRpseeApiServer;

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn build_module(handler: Arc<ExecServerHandler>) -> RpcModule<JsonRpseeExecServer> {
    JsonRpseeExecServer { handler }.into_rpc()
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct JsonRpseeExecServer {
    handler: Arc<ExecServerHandler>,
}

#[async_trait]
impl ExecServerJsonRpseeApiServer for JsonRpseeExecServer {
    async fn initialize(
        &self,
        params: InitializeParams,
    ) -> Result<InitializeResponse, ErrorObjectOwned> {
        let _ = params;
        self.handler.initialize().map_err(map_error)
    }

    async fn initialized(&self) -> Result<(), ErrorObjectOwned> {
        self.handler.initialized().map_err(internal_error)
    }

    async fn exec(&self, params: ExecParams) -> Result<ExecResponse, ErrorObjectOwned> {
        self.handler.exec(params).await.map_err(map_error)
    }

    async fn exec_read(&self, params: ReadParams) -> Result<ReadResponse, ErrorObjectOwned> {
        self.handler.exec_read(params).await.map_err(map_error)
    }

    async fn exec_write(&self, params: WriteParams) -> Result<WriteResponse, ErrorObjectOwned> {
        self.handler.exec_write(params).await.map_err(map_error)
    }

    async fn terminate(
        &self,
        params: TerminateParams,
    ) -> Result<TerminateResponse, ErrorObjectOwned> {
        self.handler.terminate(params).await.map_err(map_error)
    }

    async fn fs_read_file(
        &self,
        params: FsReadFileParams,
    ) -> Result<FsReadFileResponse, ErrorObjectOwned> {
        self.handler.fs_read_file(params).await.map_err(map_error)
    }

    async fn fs_write_file(
        &self,
        params: FsWriteFileParams,
    ) -> Result<FsWriteFileResponse, ErrorObjectOwned> {
        self.handler.fs_write_file(params).await.map_err(map_error)
    }

    async fn fs_create_directory(
        &self,
        params: FsCreateDirectoryParams,
    ) -> Result<FsCreateDirectoryResponse, ErrorObjectOwned> {
        self.handler
            .fs_create_directory(params)
            .await
            .map_err(map_error)
    }

    async fn fs_get_metadata(
        &self,
        params: FsGetMetadataParams,
    ) -> Result<FsGetMetadataResponse, ErrorObjectOwned> {
        self.handler
            .fs_get_metadata(params)
            .await
            .map_err(map_error)
    }

    async fn fs_read_directory(
        &self,
        params: FsReadDirectoryParams,
    ) -> Result<FsReadDirectoryResponse, ErrorObjectOwned> {
        self.handler
            .fs_read_directory(params)
            .await
            .map_err(map_error)
    }

    async fn fs_remove(
        &self,
        params: FsRemoveParams,
    ) -> Result<FsRemoveResponse, ErrorObjectOwned> {
        self.handler.fs_remove(params).await.map_err(map_error)
    }

    async fn fs_copy(&self, params: FsCopyParams) -> Result<FsCopyResponse, ErrorObjectOwned> {
        self.handler.fs_copy(params).await.map_err(map_error)
    }

    async fn proxy_register(
        &self,
        params: ProxyRegisterParams,
    ) -> Result<ProxyRegisterResponse, ErrorObjectOwned> {
        self.handler.proxy_register(params).await.map_err(map_error)
    }

    async fn proxy_unregister(
        &self,
        params: ProxyUnregisterParams,
    ) -> Result<ProxyUnregisterResponse, ErrorObjectOwned> {
        self.handler
            .proxy_unregister(params)
            .await
            .map_err(map_error)
    }

    async fn subscribe_exec_output(
        &self,
        pending: PendingSubscriptionSink,
        process_id: Option<ProcessId>,
    ) -> SubscriptionResult {
        let mut rx = self.handler.subscribe_process_events();
        stream_process_events(
            pending,
            move |event| match event {
                ProcessEvent::OutputDelta(notification)
                    if matches_process_filter(&notification.process_id, process_id.as_ref()) =>
                {
                    Some(notification)
                }
                _ => None,
            },
            &mut rx,
        )
        .await
    }

    async fn subscribe_exec_exited(
        &self,
        pending: PendingSubscriptionSink,
        process_id: Option<ProcessId>,
    ) -> SubscriptionResult {
        let mut rx = self.handler.subscribe_process_events();
        stream_process_events(
            pending,
            move |event| match event {
                ProcessEvent::Exited(notification)
                    if matches_process_filter(&notification.process_id, process_id.as_ref()) =>
                {
                    Some(notification)
                }
                _ => None,
            },
            &mut rx,
        )
        .await
    }

    async fn subscribe_exec_closed(
        &self,
        pending: PendingSubscriptionSink,
        process_id: Option<ProcessId>,
    ) -> SubscriptionResult {
        let mut rx = self.handler.subscribe_process_events();
        stream_process_events(
            pending,
            move |event| match event {
                ProcessEvent::Closed(notification)
                    if matches_process_filter(&notification.process_id, process_id.as_ref()) =>
                {
                    Some(notification)
                }
                _ => None,
            },
            &mut rx,
        )
        .await
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn map_error(err: JSONRPCErrorError) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(err.code as i32, err.message, err.data)
}

#[cfg_attr(not(test), allow(dead_code))]
fn internal_error(message: String) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(-32603, message, Option::<serde_json::Value>::None)
}

fn matches_process_filter(process_id: &ProcessId, filter: Option<&ProcessId>) -> bool {
    filter.is_none_or(|filter| filter == process_id)
}

async fn stream_process_events<T, F>(
    pending: PendingSubscriptionSink,
    mut select: F,
    rx: &mut tokio::sync::broadcast::Receiver<ProcessEvent>,
) -> SubscriptionResult
where
    T: serde::Serialize,
    F: FnMut(ProcessEvent) -> Option<T>,
{
    let sink = pending.accept().await?;
    loop {
        tokio::select! {
            _ = sink.closed() => break,
            received = rx.recv() => match received {
                Ok(event) => {
                    let Some(item) = select(event) else {
                        continue;
                    };
                    let message = serde_json::value::to_raw_value(&item)?;
                    if sink.send(message).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use pretty_assertions::assert_eq;

    use super::build_module;
    use crate::protocol::{InitializeParams, InitializeResponse};
    use crate::server::services::ExecServerHandler;

    #[tokio::test]
    async fn jsonrpsee_module_handles_initialize() {
        let expected_handler = ExecServerHandler::new();
        let expected = expected_handler
            .initialize()
            .expect("initialize should succeed via existing handler");
        let handler = Arc::new(ExecServerHandler::new());
        let module = build_module(handler);

        let response: InitializeResponse = module
            .call(
                "initialize",
                [InitializeParams {
                    client_name: "test-client".to_string(),
                }],
            )
            .await
            .expect("initialize should succeed");

        assert_eq!(response, expected);
    }
}
