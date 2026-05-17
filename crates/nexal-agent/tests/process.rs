#![cfg(unix)]

mod common;

use common::exec_server::{event_get, event_id, exec_server};
use nexal_agent::ExecResponse;
use nexal_agent::ProcessId;
use rmpv::Value;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_server_starts_process_over_websocket() -> anyhow::Result<()> {
    let mut server = exec_server().await?;
    let initialize_id = server
        .send_request(
            "initialize",
            serde_json::to_value(nexal_agent::InitializeParams {
                client_name: "exec-server-test".to_string(),
            })?,
        )
        .await?;
    let _ = server
        .wait_for_event(|event| event_id(event) == Some(initialize_id))
        .await?;

    let initialized_id = server
        .send_request("initialized", serde_json::Value::Null)
        .await?;
    let _ = server
        .wait_for_event(|event| event_id(event) == Some(initialized_id))
        .await?;

    let process_start_id = server
        .send_request(
            "process/start",
            serde_json::json!({
                "process_id": "proc-1",
                "argv": ["true"],
                "cwd": std::env::current_dir()?,
                "env": {},
                "tty": false,
                "arg0": null
            }),
        )
        .await?;
    let response = server
        .wait_for_event(|event| event_id(event) == Some(process_start_id))
        .await?;

    assert_eq!(event_id(&response), Some(process_start_id));
    let result = event_get(&response, "result").expect("response has result");
    let process_start_response: ExecResponse = rmpv::ext::from_value(result.clone())?;
    assert_eq!(
        process_start_response,
        ExecResponse {
            process_id: ProcessId::from("proc-1")
        }
    );

    server.shutdown().await?;
    Ok(())
}
