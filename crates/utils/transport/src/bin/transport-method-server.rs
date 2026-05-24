use futures::{SinkExt, StreamExt};
use nexal_utils_transport::agent::AgentMethod;
use nexal_utils_transport::gateway::GatewayMethod;
use rmpv::Value;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind method fixture server");
    let addr = listener.local_addr().expect("local addr");
    println!("ws://{addr}");

    let (stream, _) = listener.accept().await.expect("accept client");
    let mut ws = tokio_tungstenite::accept_async(stream)
        .await
        .expect("websocket accept");

    while let Some(msg) = ws.next().await {
        let msg = match msg {
            Ok(Message::Binary(bytes)) => bytes,
            Ok(Message::Close(_)) => break,
            Ok(Message::Ping(bytes)) => {
                let _ = ws.send(Message::Pong(bytes)).await;
                continue;
            }
            Ok(_) => continue,
            Err(_) => break,
        };

        let value: Value = match rmp_serde::from_slice(&msg) {
            Ok(value) => value,
            Err(err) => {
                eprintln!("decode request: {err}");
                continue;
            }
        };
        let id = map_get(&value, "id").cloned().unwrap_or(Value::Nil);
        let method = map_get(&value, "method")
            .and_then(Value::as_str)
            .unwrap_or("");
        let response = Value::Map(vec![
            (Value::String("id".into()), id),
            (Value::String("result".into()), sample_result(method)),
        ]);
        let bytes = rmp_serde::to_vec_named(&response).expect("encode response");
        if ws.send(Message::Binary(bytes.into())).await.is_err() {
            break;
        }
    }
}

fn map_get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.as_map()?.iter().find_map(|(k, v)| {
        if k.as_str() == Some(key) {
            Some(v)
        } else {
            None
        }
    })
}

fn sample_result(method: &str) -> Value {
    if let Some(method) = GatewayMethod::parse(method) {
        return json_value(match method {
            GatewayMethod::Hello => serde_json::json!({ "ok": true, "gateway_version": "test" }),
            GatewayMethod::SpawnAgent | GatewayMethod::AttachAgent => {
                serde_json::json!({ "agent_id": "agent-1", "container_name": "container-1" })
            }
            GatewayMethod::KillAgent
            | GatewayMethod::DetachAgent
            | GatewayMethod::UnregisterProxy
            | GatewayMethod::UnregisterStreamProxy => serde_json::json!({ "ok": true }),
            GatewayMethod::ListAgents => {
                serde_json::json!({ "agents": [{ "agent_id": "agent-1", "container_name": "container-1", "created_at_unix_ms": 1 }] })
            }
            GatewayMethod::RegisterProxy => {
                serde_json::json!({ "token": "token-1", "socket_path": "/run/nexal/proxy/test.socket" })
            }
            GatewayMethod::RegisterStreamProxy => {
                serde_json::json!({ "listen_addr": "127.0.0.1:12345" })
            }
            GatewayMethod::AgentInvoke => serde_json::json!({ "ok": true }),
        });
    }
    if let Some(method) = AgentMethod::parse(method) {
        return json_value(match method {
            AgentMethod::Initialize => {
                serde_json::json!({ "default_shell": "/bin/bash", "cwd": "/workspace" })
            }
            AgentMethod::Initialized => serde_json::Value::Null,
            AgentMethod::ProcessStart => serde_json::json!({ "process_id": "p1" }),
            AgentMethod::ProcessRead => {
                serde_json::json!({ "chunks": [], "next_seq": 0, "exited": true, "exit_code": 0, "closed": true, "failure": null })
            }
            AgentMethod::ProcessWrite => serde_json::json!({ "status": "accepted" }),
            AgentMethod::ProcessTerminate => serde_json::json!({ "running": false }),
            AgentMethod::FsReadFile => serde_json::json!({ "data": [104, 105] }),
            AgentMethod::FsWriteFile
            | AgentMethod::FsCreateDirectory
            | AgentMethod::FsRemove
            | AgentMethod::FsCopy => serde_json::json!({}),
            AgentMethod::FsGetMetadata => {
                serde_json::json!({ "isDirectory": false, "isFile": true, "createdAtMs": 1, "modifiedAtMs": 2 })
            }
            AgentMethod::FsReadDirectory => {
                serde_json::json!({ "entries": [{ "fileName": "file.txt", "isDirectory": false, "isFile": true }] })
            }
            AgentMethod::ProxyRegister | AgentMethod::ProxyUnregister => {
                serde_json::json!({ "ok": true })
            }
        });
    }
    json_value(serde_json::json!({ "ok": false, "unknown": method }))
}

fn json_value(value: serde_json::Value) -> Value {
    let bytes = rmp_serde::to_vec_named(&value).expect("json to msgpack");
    rmp_serde::from_slice(&bytes).expect("msgpack to value")
}
