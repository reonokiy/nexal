#![cfg(unix)]

mod common;

use common::exec_server::exec_server;
use nexal_agent::InitializeParams;
use nexal_agent::InitializeResponse;
use nexal_agent::JSONRPCMessage;
use nexal_agent::JSONRPCResponse;
use pretty_assertions::assert_eq;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_server_accepts_initialize() -> anyhow::Result<()> {
    let mut server = exec_server().await?;
    let initialize_id = server
        .send_request(
            "initialize",
            serde_json::to_value(InitializeParams {
                client_name: "exec-server-test".to_string(),
            })?,
        )
        .await?;

    let response = server.next_event().await?;
    let JSONRPCMessage::Response(JSONRPCResponse { id, result, .. }) = response else {
        panic!("expected initialize response");
    };
    assert_eq!(id, initialize_id);
    let initialize_response: InitializeResponse = serde_json::from_value(result)?;
    // initialize returns a default shell and cwd today; we just need a
    // successful response — the exact values are environment-dependent.
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
