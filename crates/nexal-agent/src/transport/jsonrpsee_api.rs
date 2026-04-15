use jsonrpsee::core::SubscriptionResult;
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::types::ErrorObjectOwned;

use crate::ProcessId;
use crate::protocol::{
    ExecClosedNotification, ExecExitedNotification, ExecOutputDeltaNotification, ExecParams,
    ExecResponse, FsCopyParams, FsCopyResponse, FsCreateDirectoryParams, FsCreateDirectoryResponse,
    FsGetMetadataParams, FsGetMetadataResponse, FsReadDirectoryParams, FsReadDirectoryResponse,
    FsReadFileParams, FsReadFileResponse, FsRemoveParams, FsRemoveResponse, FsWriteFileParams,
    FsWriteFileResponse, InitializeParams, InitializeResponse, ProxyRegisterParams,
    ProxyRegisterResponse, ProxyUnregisterParams, ProxyUnregisterResponse, ReadParams,
    ReadResponse, TerminateParams, TerminateResponse, WriteParams, WriteResponse,
};

#[rpc(server, client)]
pub trait ExecServerJsonRpseeApi {
    #[method(name = "initialize")]
    async fn initialize(
        &self,
        params: InitializeParams,
    ) -> Result<InitializeResponse, ErrorObjectOwned>;

    #[method(name = "initialized")]
    async fn initialized(&self) -> Result<(), ErrorObjectOwned>;

    #[method(name = "process/start")]
    async fn exec(&self, params: ExecParams) -> Result<ExecResponse, ErrorObjectOwned>;

    #[method(name = "process/read")]
    async fn exec_read(&self, params: ReadParams) -> Result<ReadResponse, ErrorObjectOwned>;

    #[method(name = "process/write")]
    async fn exec_write(&self, params: WriteParams) -> Result<WriteResponse, ErrorObjectOwned>;

    #[method(name = "process/terminate")]
    async fn terminate(
        &self,
        params: TerminateParams,
    ) -> Result<TerminateResponse, ErrorObjectOwned>;

    #[method(name = "fs/read_file")]
    async fn fs_read_file(
        &self,
        params: FsReadFileParams,
    ) -> Result<FsReadFileResponse, ErrorObjectOwned>;

    #[method(name = "fs/write_file")]
    async fn fs_write_file(
        &self,
        params: FsWriteFileParams,
    ) -> Result<FsWriteFileResponse, ErrorObjectOwned>;

    #[method(name = "fs/create_directory")]
    async fn fs_create_directory(
        &self,
        params: FsCreateDirectoryParams,
    ) -> Result<FsCreateDirectoryResponse, ErrorObjectOwned>;

    #[method(name = "fs/get_metadata")]
    async fn fs_get_metadata(
        &self,
        params: FsGetMetadataParams,
    ) -> Result<FsGetMetadataResponse, ErrorObjectOwned>;

    #[method(name = "fs/read_directory")]
    async fn fs_read_directory(
        &self,
        params: FsReadDirectoryParams,
    ) -> Result<FsReadDirectoryResponse, ErrorObjectOwned>;

    #[method(name = "fs/remove")]
    async fn fs_remove(&self, params: FsRemoveParams)
    -> Result<FsRemoveResponse, ErrorObjectOwned>;

    #[method(name = "fs/copy")]
    async fn fs_copy(&self, params: FsCopyParams) -> Result<FsCopyResponse, ErrorObjectOwned>;

    #[method(name = "proxy/register")]
    async fn proxy_register(
        &self,
        params: ProxyRegisterParams,
    ) -> Result<ProxyRegisterResponse, ErrorObjectOwned>;

    #[method(name = "proxy/unregister")]
    async fn proxy_unregister(
        &self,
        params: ProxyUnregisterParams,
    ) -> Result<ProxyUnregisterResponse, ErrorObjectOwned>;

    #[subscription(
        name = "process/subscribe_output" => "process/output",
        unsubscribe = "process/unsubscribe_output",
        item = ExecOutputDeltaNotification
    )]
    async fn subscribe_exec_output(&self, process_id: Option<ProcessId>) -> SubscriptionResult;

    #[subscription(
        name = "process/subscribe_exited" => "process/exited",
        unsubscribe = "process/unsubscribe_exited",
        item = ExecExitedNotification
    )]
    async fn subscribe_exec_exited(&self, process_id: Option<ProcessId>) -> SubscriptionResult;

    #[subscription(
        name = "process/subscribe_closed" => "process/closed",
        unsubscribe = "process/unsubscribe_closed",
        item = ExecClosedNotification
    )]
    async fn subscribe_exec_closed(&self, process_id: Option<ProcessId>) -> SubscriptionResult;
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use jsonrpsee::ws_client::WsClientBuilder;
    use tokio::time::timeout;

    use super::ExecServerJsonRpseeApiClient;
    use crate::ProcessId;
    use crate::protocol::{ExecParams, InitializeParams};
    use crate::server::start_server;

    #[tokio::test]
    async fn jsonrpsee_client_talks_to_custom_exec_server() {
        let (addr, handle) = start_server("127.0.0.1:0".parse().expect("socket addr should parse"))
            .await
            .expect("jsonrpsee server should start");

        let client = WsClientBuilder::default()
            .build(format!("ws://{addr}"))
            .await
            .expect("jsonrpsee client should connect");

        let mut exited = client
            .subscribe_exec_exited(None)
            .await
            .expect("exited subscription should succeed");

        let init = client
            .initialize(InitializeParams {
                client_name: "jsonrpsee-test".to_string(),
            })
            .await
            .expect("initialize should succeed");
        assert!(init.cwd.is_some(), "initialize should return cwd");

        client
            .initialized()
            .await
            .expect("initialized notification should succeed");

        let process_id = ProcessId::from("jsonrpsee-exit");
        client
            .exec(ExecParams {
                process_id: process_id.clone(),
                argv: vec![
                    "/bin/sh".to_string(),
                    "-lc".to_string(),
                    "exit 23".to_string(),
                ],
                cwd: std::env::current_dir().expect("current dir should resolve"),
                env: HashMap::new(),
                tty: false,
                arg0: None,
            })
            .await
            .expect("exec should succeed");

        let notification = timeout(Duration::from_secs(5), exited.next())
            .await
            .expect("exit notification should arrive before timeout")
            .expect("exit subscription should stay open")
            .expect("exit notification should deserialize");
        assert_eq!(notification.process_id, process_id);
        assert_eq!(notification.exit_code, 23);

        drop(exited);
        drop(client);
        handle.stop().expect("server should stop");
    }
}
