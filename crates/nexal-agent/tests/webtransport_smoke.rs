//! Minimal WebTransport smoke tests.
#![cfg(unix)]

mod common;

use anyhow::Result;
use nexal_utils_json_transport::{JsonMessageConnection, JsonMessageConnectionEvent};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::time::{timeout, Duration};
use wtransport::{ClientConfig, Endpoint};

use common::exec_server::exec_server_url_only;

/// Raw wtransport send/recv — bypasses JsonMessageConnection entirely.
#[tokio::test]
async fn raw_webtransport_initialize() -> Result<()> {
    let server = exec_server_url_only().await?;
    let url = server.websocket_url();

    let config = ClientConfig::builder()
        .with_bind_default()
        .with_no_cert_validation()
        .build();
    let endpoint = Endpoint::client(config)?;
    let session = timeout(Duration::from_secs(3), endpoint.connect(url)).await??;
    let opening = timeout(Duration::from_secs(3), session.open_bi()).await??;
    let (mut send, recv) = timeout(Duration::from_secs(3), opening).await??;

    let mut lines = BufReader::new(recv).lines();

    // initialize
    let msg = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"client_name":"test"}}"#;
    send.write_all(format!("{msg}\n").as_bytes()).await?;
    send.flush().await?;

    let line = timeout(Duration::from_secs(3), lines.next_line())
        .await??
        .unwrap();
    let resp: Value = serde_json::from_str(&line)?;
    assert_eq!(resp["id"], 1);
    assert!(resp["result"]["cwd"].is_string());
    eprintln!("initialize OK: {line}");

    // initialized
    let msg2 = r#"{"jsonrpc":"2.0","id":2,"method":"initialized","params":null}"#;
    send.write_all(format!("{msg2}\n").as_bytes()).await?;
    send.flush().await?;

    let line2 = timeout(Duration::from_secs(3), lines.next_line())
        .await??
        .unwrap();
    eprintln!("initialized OK: {line2}");

    // process/start
    let cwd = std::env::current_dir()?.to_string_lossy().to_string();
    let exec_msg = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"process/start","params":{{"process_id":"p1","argv":["/bin/echo","hello"],"cwd":"{}","env":{{}},"tty":false,"arg0":null}}}}"#,
        cwd
    );
    send.write_all(format!("{exec_msg}\n").as_bytes()).await?;
    send.flush().await?;
    eprintln!("sent process/start");

    let line3 = timeout(Duration::from_secs(3), lines.next_line())
        .await??
        .unwrap();
    eprintln!("process/start response: {line3}");

    Ok(())
}

/// Through JsonMessageConnection — same path as ExecServerClient.
#[tokio::test]
async fn json_message_connection_initialize() -> Result<()> {
    let server = exec_server_url_only().await?;
    let url = server.websocket_url();

    let config = ClientConfig::builder()
        .with_bind_default()
        .with_no_cert_validation()
        .build();
    let endpoint = Endpoint::client(config)?;
    let session = timeout(Duration::from_secs(3), endpoint.connect(url)).await??;
    let opening = timeout(Duration::from_secs(3), session.open_bi()).await??;
    let streams = timeout(Duration::from_secs(3), opening).await??;
    let bi: wtransport::stream::BiStream = streams.into();

    let conn = JsonMessageConnection::<Value>::from_webtransport(bi, "test".to_string());
    let (write_tx, mut incoming_rx, _tasks) = conn.into_parts();

    // Send initialize.
    write_tx
        .send(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {"client_name": "test"}
        }))
        .await?;
    eprintln!("sent initialize via JsonMessageConnection");

    // Read response.
    let event = timeout(Duration::from_secs(3), incoming_rx.recv())
        .await?
        .unwrap();
    match event {
        JsonMessageConnectionEvent::Message(value) => {
            eprintln!("got response: {value}");
            assert_eq!(value["id"], 1);
            assert!(value["result"]["cwd"].is_string());
        }
        other => panic!("unexpected event: {other:?}"),
    }

    // Send initialized.
    write_tx
        .send(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "initialized",
            "params": null
        }))
        .await?;

    let event2 = timeout(Duration::from_secs(3), incoming_rx.recv())
        .await?
        .unwrap();
    match event2 {
        JsonMessageConnectionEvent::Message(value) => {
            eprintln!("initialized response: {value}");
            assert_eq!(value["id"], 2);
        }
        other => panic!("unexpected event: {other:?}"),
    }

    eprintln!("all OK via JsonMessageConnection!");
    Ok(())
}

/// Full ExecServerClient::connect_webtransport path.
#[tokio::test]
async fn exec_server_client_webtransport() -> Result<()> {
    use nexal_agent::ExecServerClient;
    use nexal_agent::ExecServerClientConnectOptions;
    use nexal_agent::RemoteExecServerConnectArgs;

    let server = exec_server_url_only().await?;
    let url = server.websocket_url().to_string();
    eprintln!("connecting ExecServerClient to {url}");

    let client = ExecServerClient::connect_webtransport(RemoteExecServerConnectArgs {
        url: url.clone(),
        client_name: "smoke-test".to_string(),
        connect_timeout: Duration::from_secs(5),
        initialize_timeout: Duration::from_secs(5),
    })
    .await?;

    eprintln!("connected! init_response: {:?}", client.init_response());
    assert!(client.init_response().cwd.is_some());
    Ok(())
}

/// Single client, exec + read.
#[tokio::test]
async fn exec_server_client_exec_and_read() -> Result<()> {
    use nexal_agent::{ExecParams, ExecServerClient, ProcessId, ReadParams, RemoteExecServerConnectArgs};
    let _ = tracing_subscriber::fmt().with_env_filter("debug").try_init();

    let server = exec_server_url_only().await?;
    let url = server.websocket_url().to_string();
    eprintln!("connecting single client to {url}");

    let client = ExecServerClient::connect_webtransport(
        RemoteExecServerConnectArgs {
            url,
            client_name: "smoke-exec".to_string(),
            connect_timeout: Duration::from_secs(5),
            initialize_timeout: Duration::from_secs(5),
        },
    ).await?;
    eprintln!("connected, calling exec...");

    let exec_resp = client.exec(ExecParams {
        process_id: ProcessId::from("smoke-proc"),
        argv: vec!["/bin/echo".to_string(), "hello".to_string()],
        cwd: std::env::current_dir()?,
        env: Default::default(),
        tty: false,
        arg0: None,
    }).await?;
    eprintln!("exec ok: {:?}", exec_resp);

    tokio::time::sleep(Duration::from_millis(300)).await;

    let read_resp = client.read(ReadParams {
        process_id: ProcessId::from("smoke-proc"),
        after_seq: None,
        max_bytes: Some(4096),
        wait_ms: Some(1000),
    }).await?;
    eprintln!("read: exited={} exit_code={:?} chunks={}", read_resp.exited, read_resp.exit_code, read_resp.chunks.len());
    Ok(())
}
