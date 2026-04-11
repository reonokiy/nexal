use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use sqlx::{Column, Row};
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

/// One row from `cron_jobs`.
#[derive(Debug, Clone)]
pub struct CronJobRecord {
    pub id: String,
    pub label: String,
    pub schedule: String,
    pub message: String,
    pub target_channel: String,
    pub target_chat_id: String,
    pub context: String,
    pub enabled: bool,
    pub last_run_at: Option<i64>,
    pub created_at: i64,
}

/// SQLite-backed store for bot sessions, messages, and tool calls.
#[derive(Clone)]
pub struct StateDb {
    pool: SqlitePool,
    /// Separate read-only pool for untrusted queries (DB proxy).
    ro_pool: SqlitePool,
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

        // Open a separate read-only connection pool for untrusted queries.
        let ro_opts = SqliteConnectOptions::new()
            .filename(db_path)
            .read_only(true);

        let ro_pool = SqlitePool::connect_with(ro_opts)
            .await
            .context("opening bot-state sqlite (read-only)")?;

        info!("bot-state db opened at {}", db_path.display());
        Ok(Self { pool, ro_pool })
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

    // ── cron jobs ──────────────────────────────────────────────────────────

    pub async fn list_cron_jobs(&self) -> anyhow::Result<Vec<CronJobRecord>> {
        let rows = sqlx::query(
            "SELECT id, label, schedule, message, target_channel, target_chat_id,
                    context, enabled, last_run_at, created_at
             FROM cron_jobs ORDER BY created_at",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| CronJobRecord {
                id: r.get("id"),
                label: r.get("label"),
                schedule: r.get("schedule"),
                message: r.get("message"),
                target_channel: r.get("target_channel"),
                target_chat_id: r.get("target_chat_id"),
                context: r.get("context"),
                enabled: r.get::<i32, _>("enabled") != 0,
                last_run_at: r.get("last_run_at"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    pub async fn create_cron_job(&self, job: &CronJobRecord) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO cron_jobs (id, label, schedule, message, target_channel,
                                    target_chat_id, context, enabled, last_run_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&job.id)
        .bind(&job.label)
        .bind(&job.schedule)
        .bind(&job.message)
        .bind(&job.target_channel)
        .bind(&job.target_chat_id)
        .bind(&job.context)
        .bind(job.enabled as i32)
        .bind(job.last_run_at)
        .bind(job.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_cron_job_last_run(&self, id: &str, last_run_at: i64) -> anyhow::Result<()> {
        sqlx::query("UPDATE cron_jobs SET last_run_at = ? WHERE id = ?")
            .bind(last_run_at)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_cron_job(&self, id: &str) -> anyhow::Result<bool> {
        let result = sqlx::query("DELETE FROM cron_jobs WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    // ── tool calls ────────────────────────────────────────────────────────

    /// Execute a read-only SQL query and return results as JSON-compatible rows.
    ///
    /// Uses a separate read-only SQLite connection pool so that even if the
    /// text-based validation is bypassed, the database engine will reject writes.
    /// Only SELECT, WITH, PRAGMA, and EXPLAIN statements are allowed.
    pub async fn query_readonly(
        &self,
        sql: &str,
        params: &[String],
    ) -> anyhow::Result<(Vec<String>, Vec<Vec<serde_json::Value>>)> {
        use sqlx::Row;

        // Defense-in-depth: text-based validation on top of read-only connection.
        let stripped = sql.trim_start().to_lowercase();
        if !stripped.starts_with("select")
            && !stripped.starts_with("with")
            && !stripped.starts_with("pragma")
            && !stripped.starts_with("explain")
        {
            anyhow::bail!("Only SELECT / WITH / PRAGMA / EXPLAIN queries are allowed");
        }

        // Block ATTACH DATABASE — could be used to open other files.
        if stripped.contains("attach") {
            anyhow::bail!("ATTACH is not allowed");
        }

        let mut query = sqlx::query(sql);
        for p in params {
            query = query.bind(p);
        }

        // Use read-only pool — SQLite enforces read-only at the connection level.
        let rows = query.fetch_all(&self.ro_pool).await?;

        if rows.is_empty() {
            return Ok((vec![], vec![]));
        }

        // Extract column names from first row
        let columns: Vec<String> = rows[0]
            .columns()
            .iter()
            .map(|c| c.name().to_string())
            .collect();

        // Convert rows to JSON values
        let mut result_rows = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut values = Vec::with_capacity(columns.len());
            for (i, _col) in columns.iter().enumerate() {
                // Try integer first, then float, then string
                let val: serde_json::Value =
                    if let Ok(v) = row.try_get::<i64, _>(i) {
                        serde_json::Value::Number(v.into())
                    } else if let Ok(v) = row.try_get::<f64, _>(i) {
                        serde_json::json!(v)
                    } else if let Ok(v) = row.try_get::<String, _>(i) {
                        serde_json::Value::String(v)
                    } else if let Ok(v) = row.try_get::<Option<String>, _>(i) {
                        match v {
                            Some(s) => serde_json::Value::String(s),
                            None => serde_json::Value::Null,
                        }
                    } else {
                        serde_json::Value::Null
                    };
                values.push(val);
            }
            result_rows.push(values);
        }

        Ok((columns, result_rows))
    }

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
