//! End-to-end tests for the WS `Transport` + `Connection` layer.
//!
//! Mirrors the TS `packages/transport/src/rust-interop.test.ts` /
//! `method-matrix.test.ts` smoke coverage.

use std::time::Duration;

use nexal_utils_transport::WireError;
use nexal_utils_transport::connect::{
    create_accepted_websocket_connection, create_websocket_connection,
};
use nexal_utils_transport::transport::{HeartbeatOptions, TransportOptions};
use rmpv::Value;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

async fn bind_server() -> (std::net::SocketAddr, TcpListener) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    (addr, listener)
}

#[tokio::test]
async fn request_response_roundtrip() {
    let (addr, listener) = bind_server().await;

    // Server accepts one connection, handles `echo` requests.
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
        let conn = create_accepted_websocket_connection(ws, TransportOptions::default());

        conn.connection
            .handle_request("echo", |params| async move { Ok(params) })
            .await;

        // Hold the connection alive for the test duration.
        tokio::time::sleep(Duration::from_secs(2)).await;
    });

    let url = format!("ws://{addr}");
    let client = create_websocket_connection(url, TransportOptions::default())
        .await
        .unwrap();

    let res = client
        .connection
        .request("echo", Some(Value::String("hello".into())))
        .await
        .unwrap();
    assert_eq!(res, Value::String("hello".into()));

    client.connection.close().await;
    server.abort();
}

#[tokio::test]
async fn request_returns_wire_error() {
    let (addr, listener) = bind_server().await;

    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
        let conn = create_accepted_websocket_connection(ws, TransportOptions::default());
        conn.connection
            .handle_request("boom", |_| async {
                Err::<Value, _>(WireError {
                    code: 42,
                    message: "kapow".into(),
                    data: None,
                })
            })
            .await;
        tokio::time::sleep(Duration::from_secs(2)).await;
    });

    let url = format!("ws://{addr}");
    let client = create_websocket_connection(url, TransportOptions::default())
        .await
        .unwrap();

    let err = client.connection.request("boom", None).await.unwrap_err();
    match err {
        nexal_utils_transport::connection::RequestError::Wire(w) => {
            assert_eq!(w.code, 42);
            assert_eq!(w.message, "kapow");
        }
        other => panic!("unexpected: {other:?}"),
    }

    client.connection.close().await;
    server.abort();
}

#[tokio::test]
async fn notifications_are_delivered() {
    let (addr, listener) = bind_server().await;

    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
        let conn = create_accepted_websocket_connection(ws, TransportOptions::default());
        // After a brief wait, push two notifications to the client.
        tokio::time::sleep(Duration::from_millis(100)).await;
        conn.connection.notify("ping", Some(Value::from(1u32)));
        conn.connection.notify("ping", Some(Value::from(2u32)));
        tokio::time::sleep(Duration::from_secs(2)).await;
    });

    let url = format!("ws://{addr}");
    let client = create_websocket_connection(url, TransportOptions::default())
        .await
        .unwrap();

    let (tx, mut rx) = mpsc::unbounded_channel();
    client
        .connection
        .on("ping", move |params| {
            let _ = tx.send(params);
        })
        .await;

    let first = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .unwrap()
        .unwrap();
    let second = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first, Value::from(1u32));
    assert_eq!(second, Value::from(2u32));

    client.connection.close().await;
    server.abort();
}

#[tokio::test]
async fn streams_are_multiplexed() {
    let (addr, listener) = bind_server().await;

    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
        let conn = create_accepted_websocket_connection(ws, TransportOptions::default());
        // Register a request handler on the same stream id the client uses.
        let s = conn.connection.stream_async("agent-1").await;
        s.handle_request("ping", |_| async { Ok(Value::String("pong".into())) })
            .await;
        tokio::time::sleep(Duration::from_secs(2)).await;
    });

    let url = format!("ws://{addr}");
    let client = create_websocket_connection(url, TransportOptions::default())
        .await
        .unwrap();

    let s = client.connection.stream_async("agent-1").await;
    let res = s.request("ping", None).await.unwrap();
    assert_eq!(res, Value::String("pong".into()));

    client.connection.close().await;
    server.abort();
}

#[tokio::test]
async fn heartbeat_ping_pong_roundtrip() {
    let (addr, listener) = bind_server().await;

    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
        // Server uses heartbeat too, so it'll respond to client pings and vice versa.
        let _conn = create_accepted_websocket_connection(
            ws,
            TransportOptions {
                heartbeat: Some(HeartbeatOptions {
                    interval: Duration::from_millis(50),
                    timeout: Duration::from_secs(1),
                }),
                ..Default::default()
            },
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
    });

    let url = format!("ws://{addr}");
    let client = create_websocket_connection(
        url,
        TransportOptions {
            heartbeat: Some(HeartbeatOptions {
                interval: Duration::from_millis(50),
                timeout: Duration::from_secs(1),
            }),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Let several heartbeat rounds happen; the client should stay alive.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!client.transport.is_closed());

    client.connection.close().await;
    server.abort();
}
