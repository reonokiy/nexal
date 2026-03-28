use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use sqlx::Row;
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;
use tracing::info;

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// One row from `bot_sessions`.
#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub id: String,
    pub channel: String,
    pub chat_id: String,
    pub thread_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl SessionRecord {
    pub fn new(channel: &str, chat_id: &str) -> Self {
        let now = now_millis();
        Self {
            id: format!("{channel}:{chat_id}"),
            channel: channel.to_string(),
            chat_id: chat_id.to_string(),
            thread_id: None,
            created_at: now,
            updated_at: now,
        }
    }
}

/// SQLite-backed store for bot sessions, messages, and tool calls.
#[derive(Clone)]
pub struct StateDb {
    pool: SqlitePool,
}

impl StateDb {
    /// Open (or create) the SQLite database at the given path and run migrations.
    pub async fn open(db_path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("creating bot-state db directory")?;
        }

        let opts = SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true);

        let pool = SqlitePool::connect_with(opts)
            .await
            .context("opening bot-state sqlite")?;

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .context("running bot-state migrations")?;

        info!("bot-state db opened at {}", db_path.display());
        Ok(Self { pool })
    }

    // ── sessions ──────────────────────────────────────────────────────────

    pub async fn get_session(&self, id: &str) -> anyhow::Result<Option<SessionRecord>> {
        let row = sqlx::query(
            "SELECT id, channel, chat_id, thread_id, created_at, updated_at
             FROM bot_sessions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| SessionRecord {
            id: r.get("id"),
            channel: r.get("channel"),
            chat_id: r.get("chat_id"),
            thread_id: r.get("thread_id"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
        }))
    }

    pub async fn upsert_session(&self, s: &SessionRecord) -> anyhow::Result<()> {
        let now = now_millis();
        sqlx::query(
            "INSERT INTO bot_sessions (id, channel, chat_id, thread_id, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                thread_id  = excluded.thread_id,
                updated_at = ?",
        )
        .bind(&s.id)
        .bind(&s.channel)
        .bind(&s.chat_id)
        .bind(&s.thread_id)
        .bind(s.created_at)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get existing session or create a fresh one (not yet persisted).
    pub async fn get_or_create_session(
        &self,
        channel: &str,
        chat_id: &str,
    ) -> anyhow::Result<SessionRecord> {
        let id = format!("{channel}:{chat_id}");
        match self.get_session(&id).await? {
            Some(s) => Ok(s),
            None => Ok(SessionRecord::new(channel, chat_id)),
        }
    }

    // ── messages ──────────────────────────────────────────────────────────

    pub async fn save_message(
        &self,
        session_id: &str,
        sender: &str,
        role: &str,
        text: &str,
    ) -> anyhow::Result<()> {
        let ts = now_millis();
        sqlx::query(
            "INSERT INTO bot_messages (session_id, sender, role, text, timestamp)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(session_id)
        .bind(sender)
        .bind(role)
        .bind(text)
        .bind(ts)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ── tool calls ────────────────────────────────────────────────────────

    pub async fn save_tool_call(
        &self,
        session_id: &str,
        tool_call_id: &str,
        tool_name: &str,
        arguments: &str,
        output: &str,
        status: &str,
        duration_ms: Option<i64>,
    ) -> anyhow::Result<()> {
        let ts = now_millis();
        sqlx::query(
            "INSERT INTO bot_tool_calls
                (session_id, tool_call_id, tool_name, arguments, output, status, duration_ms, timestamp)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(session_id)
        .bind(tool_call_id)
        .bind(tool_name)
        .bind(arguments)
        .bind(output)
        .bind(status)
        .bind(duration_ms)
        .bind(ts)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
