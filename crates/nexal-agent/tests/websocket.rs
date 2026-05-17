#![cfg(unix)]

mod common;

use common::exec_server::{event_get, event_id, exec_server};
use nexal_agent::InitializeResponse;
use rmpv::Value;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_server_reports_unknown_method_and_keeps_running() -> anyhow::Result<()> {
    let mut server = exec_server().await?;

    // Send an unknown method — expect an error response, not a hang.
    server
        .send_request("unknown_method", serde_json::Value::Null)
        .await?;
    let response = server.next_event().await?;
    // error responses have an "error" key
    assert!(event_get(&response, "error").is_some());

    // After error, initialize still works — server keeps running.
    let initialize_id = server
        .send_request(
            "initialize",
            serde_json::to_value(nexal_agent::InitializeParams {
                client_name: "test".to_string(),
            })?,
        )
        .await?;
    let response = server.next_event().await?;
    assert_eq!(event_id(&response), Some(initialize_id));

    let result = event_get(&response, "result").expect("has result");
    let _: InitializeResponse = rmpv::ext::from_value(result.clone())?;

    server.shutdown().await?;
    Ok(())
}
