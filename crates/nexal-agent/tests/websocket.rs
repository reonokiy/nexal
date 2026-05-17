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
async fn exec_server_reports_unknown_method_and_keeps_running() -> anyhow::Result<()> {
    let mut server = exec_server().await?;

    // Send an unknown method to verify error handling.
    let bad_req_id = server
        .send_request("invalid_method", serde_json::Value::Null)
        .await?;

    let response = server
        .wait_for_event(|event| matches!(event, JSONRPCMessage::Error(_)))
        .await?;
    let JSONRPCMessage::Error(JSONRPCError { id, error, .. }) = response else {
        panic!("expected error response");
    };
    assert_eq!(id, bad_req_id);
    assert_eq!(error.code, -32601);

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
        panic!("expected initialize response after error");
    };
    assert_eq!(id, initialize_id);
    let initialize_response: InitializeResponse = rmpv::ext::from_value(result)?;
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
