//! Host-side DB query proxy — structured HTTP API for skill scripts.
//!
//! Exposes predefined query endpoints instead of raw SQL.
//! Each endpoint accepts typed filter parameters and returns JSON results.
//! No SQL injection surface.
//!
//! Endpoints:
//!   POST /toollog/query   — list tool calls with filters
//!   POST /toollog/stats   — tool call statistics
//!   POST /chatlog/query   — list messages with filters
//!   POST /chatlog/stats   — message statistics

use std::path::Path;
use std::sync::Arc;

use nexal_state::StateDb;
use serde::Deserialize;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tracing::{debug, error, info, warn};

const MAX_LIMIT: i64 = 1000;

/// Start the DB API proxy on a Unix socket at `{workspace}/agents/proxy/nexal-api`.
pub async fn start_db_proxy(
    workspace: &Path,
    db: Arc<StateDb>,
) -> tokio::task::JoinHandle<()> {
    let proxy_dir = workspace.join("agents").join("proxy");
    let _ = tokio::fs::create_dir_all(&proxy_dir).await;

    let sock_path = proxy_dir.join("nexal-api");
    let _ = tokio::fs::remove_file(&sock_path).await;

    let listener = match UnixListener::bind(&sock_path) {
        Ok(l) => l,
        Err(e) => {
            error!(path = %sock_path.display(), "failed to bind db api socket: {e}");
            return tokio::spawn(async {});
        }
    };

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o777));
    }

    info!(path = %sock_path.display(), "db api proxy started");

    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(s) => s,
                Err(e) => {
                    warn!("db api accept error: {e}");
                    continue;
                }
            };

            let db = Arc::clone(&db);
            tokio::spawn(async move {
                if let Err(e) = handle_connection(stream, &db).await {
                    debug!("db api connection error: {e}");
                }
            });
        }
    })
}

// ── Request types ────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct ToollogQueryParams {
    channel: Option<String>,
    chat_id: Option<String>,
    tool_name: Option<String>,
    status: Option<String>,
    since: Option<String>,
    until: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Deserialize, Default)]
struct ChatlogQueryParams {
    channel: Option<String>,
    chat_id: Option<String>,
    sender: Option<String>,
    role: Option<String>,
    since: Option<String>,
    until: Option<String>,
    search: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

// ── Connection handler ───────────────────────────────────────────────────

async fn handle_connection(
    stream: tokio::net::UnixStream,
    db: &StateDb,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    // Read request line
    let mut request_line = String::new();
    reader.read_line(&mut request_line).await?;
    let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
    if parts.len() < 2 {
        return write_response(&mut writer, 400, &json!({"error": "bad request"})).await;
    }
    let method = parts[0];
    let path = parts[1];

    if method != "POST" {
        return write_response(&mut writer, 405, &json!({"error": "use POST"})).await;
    }

    // Read headers
    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        if line.trim().is_empty() {
            break;
        }
        if let Some(val) = line
            .strip_prefix("Content-Length:")
            .or_else(|| line.strip_prefix("content-length:"))
        {
            content_length = val.trim().parse().unwrap_or(0);
        }
    }

    // Read body
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body).await?;
    }

    // Route to handler
    let result = match path {
        "/toollog/query" => {
            let params: ToollogQueryParams = parse_body(&body)?;
            handle_toollog_query(db, params).await
        }
        "/toollog/stats" => handle_toollog_stats(db).await,
        "/chatlog/query" => {
            let params: ChatlogQueryParams = parse_body(&body)?;
            handle_chatlog_query(db, params).await
        }
        "/chatlog/stats" => handle_chatlog_stats(db).await,
        _ => {
            return write_response(&mut writer, 404, &json!({
                "error": "unknown endpoint",
                "endpoints": ["/toollog/query", "/toollog/stats", "/chatlog/query", "/chatlog/stats"]
            })).await;
        }
    };

    match result {
        Ok(data) => write_response(&mut writer, 200, &data).await,
        Err(e) => write_response(&mut writer, 400, &json!({"error": format!("{e}")})).await,
    }
}

fn parse_body<T: serde::de::DeserializeOwned + Default>(body: &[u8]) -> anyhow::Result<T> {
    if body.is_empty() {
        return Ok(T::default());
    }
    Ok(serde_json::from_slice(body)?)
}

// ── Toollog handlers ─────────────────────────────────────────────────────

async fn handle_toollog_query(
    db: &StateDb,
    p: ToollogQueryParams,
) -> anyhow::Result<serde_json::Value> {
    let limit = p.limit.unwrap_or(50).min(MAX_LIMIT);
    let offset = p.offset.unwrap_or(0).max(0);

    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<String> = Vec::new();

    if let Some(ref v) = p.channel {
        clauses.push("session_id LIKE ? || ':%'".into());
        params.push(v.clone());
    }
    if let Some(ref v) = p.chat_id {
        clauses.push("session_id LIKE '%:' || ?".into());
        params.push(v.clone());
    }
    if let Some(ref v) = p.tool_name {
        clauses.push("tool_name = ?".into());
        params.push(v.clone());
    }
    if let Some(ref v) = p.status {
        clauses.push("status = ?".into());
        params.push(v.clone());
    }
    if let Some(ref v) = p.since {
        clauses.push("timestamp >= ?".into());
        params.push(v.clone());
    }
    if let Some(ref v) = p.until {
        clauses.push("timestamp <= ?".into());
        params.push(v.clone());
    }

    let where_clause = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    let sql = format!(
        "SELECT * FROM bot_tool_calls{where_clause} ORDER BY timestamp DESC, id DESC LIMIT ? OFFSET ?"
    );
    params.push(limit.to_string());
    params.push(offset.to_string());

    let (columns, rows) = db.query_readonly(&sql, &params).await?;
    let results: Vec<serde_json::Value> = rows
        .into_iter()
        .rev()
        .map(|row| {
            columns
                .iter()
                .zip(row)
                .map(|(k, v)| (k.clone(), v))
                .collect::<serde_json::Map<String, serde_json::Value>>()
                .into()
        })
        .collect();

    Ok(json!(results))
}

async fn handle_toollog_stats(db: &StateDb) -> anyhow::Result<serde_json::Value> {
    let (_, rows) = db
        .query_readonly("SELECT COUNT(*) as total FROM bot_tool_calls", &[])
        .await?;
    let total = rows.first().and_then(|r| r.first()).cloned().unwrap_or(json!(0));

    let (cols, rows) = db
        .query_readonly(
            "SELECT tool_name, COUNT(*) as count, \
             SUM(CASE WHEN status='error' THEN 1 ELSE 0 END) as errors, \
             ROUND(AVG(duration_ms)) as avg_duration_ms \
             FROM bot_tool_calls GROUP BY tool_name ORDER BY count DESC",
            &[],
        )
        .await?;
    let by_tool = rows_to_dicts(&cols, rows);

    let (cols, rows) = db
        .query_readonly(
            "SELECT SUBSTR(session_id, 1, INSTR(session_id, ':') - 1) as channel, \
             COUNT(*) as count FROM bot_tool_calls GROUP BY channel ORDER BY count DESC",
            &[],
        )
        .await?;
    let by_channel = rows_to_dicts(&cols, rows);

    Ok(json!({
        "total_tool_calls": total,
        "tool_call_stats": by_tool,
        "tool_calls_by_channel": by_channel,
    }))
}

// ── Chatlog handlers ─────────────────────────────────────────────────────

async fn handle_chatlog_query(
    db: &StateDb,
    p: ChatlogQueryParams,
) -> anyhow::Result<serde_json::Value> {
    let limit = p.limit.unwrap_or(50).min(MAX_LIMIT);
    let offset = p.offset.unwrap_or(0).max(0);

    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<String> = Vec::new();

    if let Some(ref v) = p.channel {
        clauses.push("session_id LIKE ? || ':%'".into());
        params.push(v.clone());
    }
    if let Some(ref v) = p.chat_id {
        clauses.push("session_id LIKE '%:' || ?".into());
        params.push(v.clone());
    }
    if let Some(ref v) = p.sender {
        clauses.push("sender = ?".into());
        params.push(v.clone());
    }
    if let Some(ref v) = p.role {
        clauses.push("role = ?".into());
        params.push(v.clone());
    }
    if let Some(ref v) = p.since {
        clauses.push("timestamp >= ?".into());
        params.push(v.clone());
    }
    if let Some(ref v) = p.until {
        clauses.push("timestamp <= ?".into());
        params.push(v.clone());
    }
    if let Some(ref v) = p.search {
        clauses.push("text LIKE '%' || ? || '%'".into());
        params.push(v.clone());
    }

    let where_clause = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    let sql = format!(
        "SELECT * FROM bot_messages{where_clause} ORDER BY timestamp DESC, id DESC LIMIT ? OFFSET ?"
    );
    params.push(limit.to_string());
    params.push(offset.to_string());

    let (columns, rows) = db.query_readonly(&sql, &params).await?;
    let results: Vec<serde_json::Value> = rows
        .into_iter()
        .rev()
        .map(|row| {
            columns
                .iter()
                .zip(row)
                .map(|(k, v)| (k.clone(), v))
                .collect::<serde_json::Map<String, serde_json::Value>>()
                .into()
        })
        .collect();

    Ok(json!(results))
}

async fn handle_chatlog_stats(db: &StateDb) -> anyhow::Result<serde_json::Value> {
    let (_, rows) = db
        .query_readonly("SELECT COUNT(*) as total FROM bot_messages", &[])
        .await?;
    let total = rows.first().and_then(|r| r.first()).cloned().unwrap_or(json!(0));

    let (cols, rows) = db
        .query_readonly(
            "SELECT SUBSTR(session_id, 1, INSTR(session_id, ':') - 1) as channel, \
             role, COUNT(*) as count FROM bot_messages GROUP BY channel, role ORDER BY channel, role",
            &[],
        )
        .await?;
    let by_channel_role = rows_to_dicts(&cols, rows);

    let (cols, rows) = db
        .query_readonly(
            "SELECT sender, COUNT(*) as count FROM bot_messages \
             GROUP BY sender ORDER BY count DESC LIMIT 20",
            &[],
        )
        .await?;
    let top_senders = rows_to_dicts(&cols, rows);

    Ok(json!({
        "total_messages": total,
        "messages_by_channel_role": by_channel_role,
        "top_senders": top_senders,
    }))
}

// ── Helpers ──────────────────────────────────────────────────────────────

fn rows_to_dicts(columns: &[String], rows: Vec<Vec<serde_json::Value>>) -> Vec<serde_json::Value> {
    rows.into_iter()
        .map(|row| {
            columns
                .iter()
                .zip(row)
                .map(|(k, v)| (k.clone(), v))
                .collect::<serde_json::Map<String, serde_json::Value>>()
                .into()
        })
        .collect()
}

async fn write_response(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    status: u16,
    body: &serde_json::Value,
) -> anyhow::Result<()> {
    let body_bytes = serde_json::to_vec(body)?;
    let status_text = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let response = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body_bytes.len()
    );
    writer.write_all(response.as_bytes()).await?;
    writer.write_all(&body_bytes).await?;
    Ok(())
}
