use futures::{SinkExt, StreamExt};
use rmpv::Value;
use tokio_tungstenite::tungstenite::Message;

const METHODS: &[&str] = &[
    "gateway/hello",
    "gateway/spawn_agent",
    "gateway/kill_agent",
    "gateway/detach_agent",
    "gateway/attach_agent",
    "gateway/list_agents",
    "gateway/register_proxy",
    "gateway/unregister_proxy",
    "gateway/register_stream_proxy",
    "gateway/unregister_stream_proxy",
    "agent/invoke",
    "initialize",
    "initialized",
    "process/start",
    "process/read",
    "process/write",
    "process/terminate",
    "fs/read_file",
    "fs/write_file",
    "fs/create_directory",
    "fs/get_metadata",
    "fs/read_directory",
    "fs/remove",
    "fs/copy",
    "proxy/register",
    "proxy/unregister",
];

#[tokio::main]
async fn main() {
    let Some(url) = std::env::args().nth(1) else {
        eprintln!("usage: transport-method-client <ws-url>");
        std::process::exit(2);
    };

    let (mut ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("connect to fixture server");

    for (idx, method) in METHODS.iter().enumerate() {
        let id = format!("req-{idx}");
        let request = Value::Map(vec![
            (Value::String("id".into()), Value::String(id.clone().into())),
            (
                Value::String("method".into()),
                Value::String((*method).into()),
            ),
            (Value::String("params".into()), sample_params(method)),
        ]);
        let bytes = rmp_serde::to_vec_named(&request).expect("encode request");
        ws.send(Message::Binary(bytes.into()))
            .await
            .expect("send request");

        let response = loop {
            match ws
                .next()
                .await
                .expect("response frame")
                .expect("response ok")
            {
                Message::Binary(bytes) => {
                    break rmp_serde::from_slice::<Value>(&bytes).expect("decode response");
                }
                Message::Ping(bytes) => {
                    let _ = ws.send(Message::Pong(bytes)).await;
                }
                _ => continue,
            }
        };
        let got_id = map_get(&response, "id")
            .and_then(Value::as_str)
            .unwrap_or("");
        if got_id != id {
            eprintln!("id mismatch for {method}: expected {id}, got {got_id}");
            std::process::exit(1);
        }
        if map_get(&response, "error").is_some() || map_get(&response, "result").is_none() {
            eprintln!("bad response for {method}: {response:?}");
            std::process::exit(1);
        }
    }

    println!("ok {}", METHODS.len());
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

fn sample_params(method: &str) -> Value {
    json_value(match method {
        "gateway/hello" => {
            serde_json::json!({ "access_key": "ak", "client_name": "rust-client", "ts": 1, "nonce": "n", "signature": "s" })
        }
        "gateway/spawn_agent" => {
            serde_json::json!({ "name": "agent", "env": {}, "labels": {}, "extra_ports": [] })
        }
        "gateway/kill_agent" | "gateway/detach_agent" => {
            serde_json::json!({ "agent_id": "agent-1" })
        }
        "gateway/attach_agent" => serde_json::json!({ "container_name": "container-1" }),
        "gateway/list_agents" | "initialized" => serde_json::json!({}),
        "gateway/register_proxy" => {
            serde_json::json!({ "agent_id": "agent-1", "name": "proxy", "upstream_url": "https://example.com", "headers": {} })
        }
        "gateway/unregister_proxy" | "gateway/unregister_stream_proxy" => {
            serde_json::json!({ "agent_id": "agent-1", "name": "proxy" })
        }
        "gateway/register_stream_proxy" => {
            serde_json::json!({ "agent_id": "agent-1", "name": "tcp", "container_port": 3000 })
        }
        "agent/invoke" => {
            serde_json::json!({ "agent_id": "agent-1", "method": "initialize", "params": { "client_name": "rust-client" } })
        }
        "initialize" => serde_json::json!({ "client_name": "rust-client" }),
        "process/start" => {
            serde_json::json!({ "process_id": "p1", "argv": ["true"], "cwd": "/workspace", "env": {}, "tty": false, "arg0": null })
        }
        "process/read" => {
            serde_json::json!({ "process_id": "p1", "after_seq": 0, "max_bytes": 1024, "wait_ms": 0 })
        }
        "process/write" => serde_json::json!({ "process_id": "p1", "chunk": [104, 105] }),
        "process/terminate" => serde_json::json!({ "process_id": "p1" }),
        "fs/read_file" | "fs/get_metadata" | "fs/read_directory" => {
            serde_json::json!({ "path": "/workspace/file.txt" })
        }
        "fs/write_file" => serde_json::json!({ "path": "/workspace/file.txt", "data": [104, 105] }),
        "fs/create_directory" => serde_json::json!({ "path": "/workspace/dir", "recursive": true }),
        "fs/remove" => {
            serde_json::json!({ "path": "/workspace/file.txt", "recursive": true, "force": true })
        }
        "fs/copy" => {
            serde_json::json!({ "sourcePath": "/workspace/a", "destinationPath": "/workspace/b", "recursive": true })
        }
        "proxy/register" => {
            serde_json::json!({ "socket_path": "/run/nexal/proxy/p.socket", "upstream_url": "https://example.com", "headers": {} })
        }
        "proxy/unregister" => serde_json::json!({ "socket_path": "/run/nexal/proxy/p.socket" }),
        _ => serde_json::json!({}),
    })
}

fn json_value(value: serde_json::Value) -> Value {
    let bytes = rmp_serde::to_vec_named(&value).expect("json to msgpack");
    rmp_serde::from_slice(&bytes).expect("msgpack to value")
}
