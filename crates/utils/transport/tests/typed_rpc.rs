//! End-to-end tests for the typed client/server wrappers.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use nexal_utils_transport::agent::{
    AgentInitialize, FsReadFileParams, FsReadFileResponse, InitializeParams, InitializeResponse,
    ProcessStartParams, ProcessStartResponse,
};
use nexal_utils_transport::client::{AgentClient, GatewayAgentClient, GatewayClient};
use nexal_utils_transport::connect::{
    create_accepted_websocket_connection, create_websocket_connection,
};
use nexal_utils_transport::gateway::{AgentInvokeParams, HelloParams, HelloResponse};
use nexal_utils_transport::notifications::{PROCESS_OUTPUT, ProcessOutputNotification};
use nexal_utils_transport::server::{AgentHandlers, GatewayHandlers, serve_agent, serve_gateway};
use nexal_utils_transport::transport::TransportOptions;
use nexal_utils_transport::{RpcMethod, RpcMethodExt, WireError, from_msgpack_value, to_msgpack_value};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

async fn bind() -> (std::net::SocketAddr, TcpListener) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    (addr, listener)
}

// ── Test handler impls ──────────────────────────────────────────────

struct EchoAgent;

impl AgentHandlers for EchoAgent {
    async fn initialize(
        &self,
        p: InitializeParams,
    ) -> Result<InitializeResponse, WireError> {
        Ok(InitializeResponse {
            default_shell: Some(format!("shell-for-{}", p.client_name)),
            cwd: Some(PathBuf::from("/srv")),
        })
    }

    async fn fs_read_file(&self, p: FsReadFileParams) -> Result<FsReadFileResponse, WireError> {
        Ok(FsReadFileResponse {
            data: format!("contents of {}", p.path).into_bytes(),
        })
    }
}

struct BoomAgent;

impl AgentHandlers for BoomAgent {
    async fn process_start(
        &self,
        _p: ProcessStartParams,
    ) -> Result<ProcessStartResponse, WireError> {
        Err(WireError {
            code: 12345,
            message: "no can do".into(),
            data: None,
        })
    }
}

struct TestGateway;

impl GatewayHandlers for TestGateway {
    async fn hello(&self, p: HelloParams) -> Result<HelloResponse, WireError> {
        Ok(HelloResponse {
            ok: !p.access_key.is_empty(),
            gateway_version: "0.0.0-test".into(),
        })
    }
}

struct ProxyGateway;

impl GatewayHandlers for ProxyGateway {
    async fn agent_invoke(
        &self,
        p: AgentInvokeParams,
    ) -> Result<rmpv::Value, WireError> {
        assert_eq!(p.agent_id, "agent-xyz");
        match p.method.as_str() {
            m if m == AgentInitialize::METHOD => {
                let params: InitializeParams =
                    from_msgpack_value(p.params.unwrap()).map_err(|e| WireError {
                        code: -32602,
                        message: e,
                        data: None,
                    })?;
                let result = InitializeResponse {
                    default_shell: Some(format!("shell-for-{}", params.client_name)),
                    cwd: None,
                };
                to_msgpack_value(&result).map_err(|e| WireError {
                    code: -32603,
                    message: e,
                    data: None,
                })
            }
            other => Err(WireError {
                code: -32601,
                message: format!("unknown agent method: {other}"),
                data: None,
            }),
        }
    }
}

// ── Helper: spawn a server with given handler installer ────────────

async fn spawn_server<F, Fut>(setup: F) -> std::net::SocketAddr
where
    F: FnOnce(
            nexal_utils_transport::connect::WebSocketConnection,
            Arc<AtomicBool>,
        ) -> Fut
        + Send
        + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let (addr, listener) = bind().await;
    let alive = Arc::new(AtomicBool::new(true));
    let alive_for_server = alive.clone();
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
        let conn = create_accepted_websocket_connection(ws, TransportOptions::default());
        setup(conn, alive_for_server).await;
    });
    addr
}

// ── Tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn agent_handlers_trait_initialize_and_fs_read_file() {
    let addr = spawn_server(|conn, _alive| async move {
        serve_agent(&conn.connection, Arc::new(EchoAgent)).await;
        tokio::time::sleep(Duration::from_secs(3)).await;
    })
    .await;

    let ws = create_websocket_connection(format!("ws://{addr}"), TransportOptions::default())
        .await
        .unwrap();
    let client = AgentClient::new(ws.connection.clone());

    let init = client
        .initialize(&InitializeParams {
            client_name: "demo".into(),
        })
        .await
        .unwrap();
    assert_eq!(init.default_shell.as_deref(), Some("shell-for-demo"));
    assert_eq!(init.cwd, Some(PathBuf::from("/srv")));

    let read = client
        .fs_read_file(&FsReadFileParams {
            path: "/etc/hosts".into(),
        })
        .await
        .unwrap();
    assert_eq!(read.data, b"contents of /etc/hosts");

    ws.connection.close().await;
}

#[tokio::test]
async fn agent_handlers_default_returns_method_not_found() {
    let addr = spawn_server(|conn, _| async move {
        // EchoAgent only overrides initialize + fs_read_file. process/start
        // should fall through to the default implementation.
        serve_agent(&conn.connection, Arc::new(EchoAgent)).await;
        tokio::time::sleep(Duration::from_secs(3)).await;
    })
    .await;

    let ws = create_websocket_connection(format!("ws://{addr}"), TransportOptions::default())
        .await
        .unwrap();
    let client = AgentClient::new(ws.connection.clone());

    let err = client
        .process_start(&ProcessStartParams {
            process_id: "p-1".into(),
            argv: vec!["/bin/true".into()],
            cwd: PathBuf::from("/tmp"),
            env: Default::default(),
            tty: false,
            arg0: None,
            output_bytes_cap: None,
        })
        .await
        .unwrap_err();

    match err {
        nexal_utils_transport::connection::TypedRequestError::Request(
            nexal_utils_transport::connection::RequestError::Wire(w),
        ) => {
            assert_eq!(w.code, -32601);
            assert!(w.message.contains("process/start"));
        }
        other => panic!("unexpected: {other:?}"),
    }

    ws.connection.close().await;
}

#[tokio::test]
async fn agent_client_receives_process_output_notifications() {
    let addr = spawn_server(|conn, alive| async move {
        // Push notifications in a loop until the test signals it's done.
        let mut seq = 0u64;
        while alive.load(Ordering::SeqCst) {
            seq += 1;
            conn.connection.notify_typed(
                PROCESS_OUTPUT,
                &ProcessOutputNotification {
                    process_id: "p-1".into(),
                    seq,
                    stream: nexal_utils_transport::agent::StreamKind::Stdout,
                    chunk: b"hello".to_vec(),
                },
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
            if seq > 200 {
                break;
            }
        }
    })
    .await;

    let ws = create_websocket_connection(format!("ws://{addr}"), TransportOptions::default())
        .await
        .unwrap();
    let client = AgentClient::new(ws.connection.clone());

    let (tx, mut rx) = mpsc::unbounded_channel();
    client
        .on_process_output(move |notif| {
            let _ = tx.send(notif);
        })
        .await;

    let got = tokio::time::timeout(Duration::from_secs(3), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.process_id, "p-1");
    assert_eq!(got.chunk, b"hello");

    ws.connection.close().await;
}

#[tokio::test]
async fn gateway_handlers_hello() {
    let addr = spawn_server(|conn, _| async move {
        serve_gateway(&conn.connection, Arc::new(TestGateway)).await;
        tokio::time::sleep(Duration::from_secs(3)).await;
    })
    .await;

    let ws = create_websocket_connection(format!("ws://{addr}"), TransportOptions::default())
        .await
        .unwrap();
    let client = GatewayClient::new(ws.connection.clone());

    let resp = client
        .hello(&HelloParams {
            access_key: "ak".into(),
            client_name: "client-a".into(),
            ts: 1700000000,
            nonce: "nonce-1".into(),
            signature: "sig".into(),
        })
        .await
        .unwrap();
    assert!(resp.ok);
    assert_eq!(resp.gateway_version, "0.0.0-test");

    ws.connection.close().await;
}

#[tokio::test]
async fn gateway_agent_client_proxies_through_agent_invoke() {
    let addr = spawn_server(|conn, _| async move {
        serve_gateway(&conn.connection, Arc::new(ProxyGateway)).await;
        tokio::time::sleep(Duration::from_secs(3)).await;
    })
    .await;

    let ws = create_websocket_connection(format!("ws://{addr}"), TransportOptions::default())
        .await
        .unwrap();
    let client = GatewayAgentClient::new(ws.connection.clone(), "agent-xyz");

    let init = client
        .initialize(&InitializeParams {
            client_name: "proxied".into(),
        })
        .await
        .unwrap();
    assert_eq!(init.default_shell.as_deref(), Some("shell-for-proxied"));

    ws.connection.close().await;
}

#[tokio::test]
async fn typed_request_propagates_wire_error() {
    let addr = spawn_server(|conn, _| async move {
        serve_agent(&conn.connection, Arc::new(BoomAgent)).await;
        tokio::time::sleep(Duration::from_secs(3)).await;
    })
    .await;

    let ws = create_websocket_connection(format!("ws://{addr}"), TransportOptions::default())
        .await
        .unwrap();
    let client = AgentClient::new(ws.connection.clone());

    let err = client
        .process_start(&ProcessStartParams {
            process_id: "p-1".into(),
            argv: vec!["/bin/true".into()],
            cwd: PathBuf::from("/tmp"),
            env: Default::default(),
            tty: false,
            arg0: None,
            output_bytes_cap: None,
        })
        .await
        .unwrap_err();
    match err {
        nexal_utils_transport::connection::TypedRequestError::Request(
            nexal_utils_transport::connection::RequestError::Wire(w),
        ) => {
            assert_eq!(w.code, 12345);
            assert_eq!(w.message, "no can do");
        }
        other => panic!("unexpected: {other:?}"),
    }

    ws.connection.close().await;
}

#[tokio::test]
async fn rpc_method_ext_call_sugar() {
    // Demonstrates `AgentInitialize::call(&conn, &params)` direct usage,
    // bypassing the AgentClient wrapper entirely.
    let addr = spawn_server(|conn, _| async move {
        serve_agent(&conn.connection, Arc::new(EchoAgent)).await;
        tokio::time::sleep(Duration::from_secs(3)).await;
    })
    .await;

    let ws = create_websocket_connection(format!("ws://{addr}"), TransportOptions::default())
        .await
        .unwrap();

    let init = AgentInitialize::call(
        &ws.connection,
        &InitializeParams {
            client_name: "ext-trait".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(init.default_shell.as_deref(), Some("shell-for-ext-trait"));

    ws.connection.close().await;
}
