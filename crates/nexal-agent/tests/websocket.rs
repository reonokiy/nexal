#![cfg(unix)]

mod common;

use common::exec_server::exec_server;
use nexal_agent::InitializeParams;
use nexal_agent::InitializeResponse;
use nexal_agent::JSONRPCError;
use nexal_agent::JSONRPCMessage;
use nexal_agent::JSONRPCResponse;
use pretty_assertions::assert_eq;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_server_reports_malformed_websocket_json_and_keeps_running() -> anyhow::Result<()> {
    let mut server = exec_server().await?;
    server.send_raw_text("not-json").await?;

    let response = server
        .wait_for_event(|event| matches!(event, JSONRPCMessage::Error(_)))
        .await?;
    let JSONRPCMessage::Error(JSONRPCError { id, error, .. }) = response else {
        panic!("expected malformed-message error response");
    };
    // jsonrpsee emits a null id for parse errors (the caller's id is
    // unknown when the frame couldn't be parsed) and code -32700.
    assert_eq!(id, nexal_agent::RequestId::Null);
    assert_eq!(error.code, -32700);

    let initialize_id = server
        .send_request(
            "initialize",
            serde_json::to_value(InitializeParams {
                client_name: "exec-server-test".to_string(),
            })?,
        )
        .await?;

    let response = server
        .wait_for_event(|event| {
            matches!(
                event,
                JSONRPCMessage::Response(JSONRPCResponse { id, .. }) if id == &initialize_id
            )
        })
        .await?;
    let JSONRPCMessage::Response(JSONRPCResponse { id, result, .. }) = response else {
        panic!("expected initialize response after malformed input");
    };
    assert_eq!(id, initialize_id);
    let initialize_response: InitializeResponse = serde_json::from_value(result)?;
    // Same as `tests/initialize.rs`: any valid response works; the
    // specific default_shell/cwd values are environment-dependent.
    assert!(
        initialize_response.default_shell.is_some()
            || initialize_response.cwd.is_some()
            || initialize_response == InitializeResponse::default(),
        "unexpected initialize response: {:?}",
        initialize_response,
    );

    server.shutdown().await?;
    Ok(())
}
