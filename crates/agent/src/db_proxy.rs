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

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use hyper::body::Incoming;
use hyper::Request;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use nexal_state::StateDb;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::net::UnixListener;
use tower::Service;
use tracing::{debug, error, info, warn};

const MAX_LIMIT: i64 = 1000;

type AppState = Arc<StateDb>;

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

    let app: Router = Router::new()
        .route("/toollog/query", post(route_toollog_query))
        .route("/toollog/stats", post(route_toollog_stats))
        .route("/chatlog/query", post(route_chatlog_query))
        .route("/chatlog/stats", post(route_chatlog_stats))
        .with_state(db);

    tokio::spawn(async move { serve_unix(listener, app).await })
}

/// Serve an axum `Router` over a `UnixListener`. Each accepted connection is
/// driven by a hyper HTTP/1 connection handler running on its own task.
async fn serve_unix(listener: UnixListener, app: Router) {
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(s) => s,
            Err(e) => {
                warn!("db api accept error: {e}");
                continue;
            }
        };

        let tower_service = app.clone();
        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            let hyper_service =
                hyper::service::service_fn(move |req: Request<Incoming>| {
                    tower_service.clone().call(req)
                });
            if let Err(e) = Builder::new(TokioExecutor::new())
                .serve_connection(io, hyper_service)
                .await
            {
                debug!("db api connection error: {e}");
            }
        });
    }
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

/// Wrap a handler result into an axum response. Errors become 400s with a
/// JSON error body — matching the old hand-rolled protocol.
struct ApiResult(anyhow::Result<Value>);

impl IntoResponse for ApiResult {
    fn into_response(self) -> Response {
        match self.0 {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("{e}") })),
            )
                .into_response(),
        }
    }
}

// ── Route handlers ───────────────────────────────────────────────────────

async fn route_toollog_query(
    State(db): State<AppState>,
    body: Option<Json<ToollogQueryParams>>,
) -> ApiResult {
    let params = body.map(|Json(p)| p).unwrap_or_default();
    ApiResult(handle_toollog_query(&db, params).await)
}

async fn route_toollog_stats(State(db): State<AppState>) -> ApiResult {
    ApiResult(handle_toollog_stats(&db).await)
}

async fn route_chatlog_query(
    State(db): State<AppState>,
    body: Option<Json<ChatlogQueryParams>>,
) -> ApiResult {
    let params = body.map(|Json(p)| p).unwrap_or_default();
    ApiResult(handle_chatlog_query(&db, params).await)
}

async fn route_chatlog_stats(State(db): State<AppState>) -> ApiResult {
    ApiResult(handle_chatlog_stats(&db).await)
}

// ── Toollog handlers ─────────────────────────────────────────────────────

async fn handle_toollog_query(
    db: &StateDb,
    p: ToollogQueryParams,
) -> anyhow::Result<Value> {
    let limit = p.limit.unwrap_or(50).min(MAX_LIMIT);
    let offset = p.offset.unwrap_or(0).max(0);

    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<String> = Vec::new();

    push_filter(&mut clauses, &mut params, "session_id LIKE ? || ':%' ESCAPE '\\'", p.channel.as_ref().map(|v| like_escape(v)).as_ref());
    push_filter(&mut clauses, &mut params, "session_id LIKE '%:' || ? ESCAPE '\\'", p.chat_id.as_ref().map(|v| like_escape(v)).as_ref());
    push_filter(&mut clauses, &mut params, "tool_name = ?", p.tool_name.as_ref());
    push_filter(&mut clauses, &mut params, "status = ?", p.status.as_ref());
    push_filter(&mut clauses, &mut params, "timestamp >= ?", p.since.as_ref());
    push_filter(&mut clauses, &mut params, "timestamp <= ?", p.until.as_ref());

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
    Ok(json!(rows_to_dicts(&columns, rows.into_iter().rev().collect())))
}

async fn handle_toollog_stats(db: &StateDb) -> anyhow::Result<Value> {
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
) -> anyhow::Result<Value> {
    let limit = p.limit.unwrap_or(50).min(MAX_LIMIT);
    let offset = p.offset.unwrap_or(0).max(0);

    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<String> = Vec::new();

    push_filter(&mut clauses, &mut params, "session_id LIKE ? || ':%' ESCAPE '\\'", p.channel.as_ref().map(|v| like_escape(v)).as_ref());
    push_filter(&mut clauses, &mut params, "session_id LIKE '%:' || ? ESCAPE '\\'", p.chat_id.as_ref().map(|v| like_escape(v)).as_ref());
    push_filter(&mut clauses, &mut params, "sender = ?", p.sender.as_ref());
    push_filter(&mut clauses, &mut params, "role = ?", p.role.as_ref());
    push_filter(&mut clauses, &mut params, "timestamp >= ?", p.since.as_ref());
    push_filter(&mut clauses, &mut params, "timestamp <= ?", p.until.as_ref());
    push_filter(&mut clauses, &mut params, "text LIKE '%' || ? || '%' ESCAPE '\\'", p.search.as_ref().map(|v| like_escape(v)).as_ref());

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
    Ok(json!(rows_to_dicts(&columns, rows.into_iter().rev().collect())))
}

async fn handle_chatlog_stats(db: &StateDb) -> anyhow::Result<Value> {
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

fn push_filter(
    clauses: &mut Vec<String>,
    params: &mut Vec<String>,
    clause: &str,
    value: Option<&String>,
) {
    if let Some(v) = value {
        clauses.push(clause.to_string());
        params.push(v.clone());
    }
}

/// Escape a value for use inside a SQLite/Postgres LIKE pattern with `ESCAPE '\'`.
/// Escapes `\`, `%`, and `_` so they are treated as literals.
fn like_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' | '%' | '_' => { out.push('\\'); out.push(ch); }
            _ => out.push(ch),
        }
    }
    out
}

fn rows_to_dicts(columns: &[String], rows: Vec<Vec<Value>>) -> Vec<Value> {
    rows.into_iter()
        .map(|row| {
            columns
                .iter()
                .zip(row)
                .map(|(k, v)| (k.clone(), v))
                .collect::<serde_json::Map<String, Value>>()
                .into()
        })
        .collect()
}
