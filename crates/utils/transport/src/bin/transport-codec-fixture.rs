use nexal_utils_transport::{WireMessage, WireResponse, decode_wire_message, encode_frame};
use rmpv::Value;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(cmd) = args.next() else {
        eprintln!("usage: transport-codec-fixture <decode-request HEX|encode-response>");
        std::process::exit(2);
    };

    match cmd.as_str() {
        "decode-request" => {
            let Some(hex) = args.next() else {
                eprintln!("decode-request requires HEX");
                std::process::exit(2);
            };
            let bytes = match decode_hex(&hex) {
                Ok(bytes) => bytes,
                Err(err) => {
                    eprintln!("{err}");
                    std::process::exit(2);
                }
            };
            let message = match decode_wire_message(&bytes) {
                Ok(message) => message,
                Err(err) => {
                    eprintln!("decode failed: {err}");
                    std::process::exit(1);
                }
            };
            let json = match message {
                WireMessage::Request(req) => serde_json::json!({
                    "kind": "request",
                    "stream": req.stream,
                    "id": value_to_json(req.id),
                    "method": req.method,
                    "params": req.params.map(value_to_json),
                }),
                WireMessage::Response(resp) => serde_json::json!({
                    "kind": "response",
                    "stream": resp.stream,
                    "id": value_to_json(resp.id),
                    "result": resp.result.map(value_to_json),
                    "error": resp.error.map(|err| serde_json::json!({
                        "code": err.code,
                        "message": err.message,
                        "data": err.data.map(value_to_json),
                    })),
                }),
                WireMessage::Notification(notif) => serde_json::json!({
                    "kind": "notification",
                    "stream": notif.stream,
                    "method": notif.method,
                    "params": notif.params.map(value_to_json),
                }),
            };
            println!(
                "{}",
                serde_json::to_string(&json).unwrap_or_else(|_| "{}".into())
            );
        }
        "encode-response" => {
            let response = WireResponse::ok(
                Value::String("req-1".into()),
                Value::Map(vec![(Value::String("ok".into()), Value::Boolean(true))]),
            );
            let bytes = match encode_frame(&response) {
                Ok(bytes) => bytes,
                Err(err) => {
                    eprintln!("encode failed: {err}");
                    std::process::exit(1);
                }
            };
            println!("{}", encode_hex(&bytes));
        }
        _ => {
            eprintln!("unknown command: {cmd}");
            std::process::exit(2);
        }
    }
}

fn decode_hex(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err("hex length must be even".into());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    for i in (0..bytes.len()).step_by(2) {
        let hi = hex_value(bytes[i]).ok_or_else(|| format!("invalid hex at {i}"))?;
        let lo = hex_value(bytes[i + 1]).ok_or_else(|| format!("invalid hex at {}", i + 1))?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn value_to_json(value: Value) -> serde_json::Value {
    match value {
        Value::Nil => serde_json::Value::Null,
        Value::Boolean(v) => serde_json::Value::Bool(v),
        Value::Integer(v) => {
            if let Some(n) = v.as_i64() {
                serde_json::Value::Number(n.into())
            } else if let Some(n) = v.as_u64() {
                serde_json::Value::Number(n.into())
            } else {
                serde_json::Value::String(v.to_string())
            }
        }
        Value::F32(v) => serde_json::json!(v),
        Value::F64(v) => serde_json::json!(v),
        Value::String(v) => serde_json::Value::String(v.as_str().unwrap_or("").to_string()),
        Value::Binary(v) => serde_json::Value::Array(v.into_iter().map(|b| b.into()).collect()),
        Value::Array(v) => serde_json::Value::Array(v.into_iter().map(value_to_json).collect()),
        Value::Map(v) => {
            let mut map = serde_json::Map::new();
            for (k, v) in v {
                map.insert(k.as_str().unwrap_or("").to_string(), value_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        Value::Ext(_, _) => serde_json::Value::Null,
    }
}
