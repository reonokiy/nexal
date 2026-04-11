pub mod entity;
mod migrator;

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection,
    EntityTrait, Order, QueryFilter, QueryOrder,
};
use sea_orm_migration::MigratorTrait;
use sqlx::{Column, Row};
use sqlx::SqlitePool;
use sqlx::PgPool;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::postgres::PgConnectOptions;
use std::str::FromStr;
use tracing::info;

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

// ── Public record types ───────────────────────────────────────────────────────

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

impl From<entity::bot_session::Model> for SessionRecord {
    fn from(m: entity::bot_session::Model) -> Self {
        Self {
            id: m.id,
            channel: m.channel,
            chat_id: m.chat_id,
            thread_id: m.thread_id,
            created_at: m.created_at,
            updated_at: m.updated_at,
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

impl From<entity::cron_job::Model> for CronJobRecord {
    fn from(m: entity::cron_job::Model) -> Self {
        Self {
            id: m.id,
            label: m.label,
            schedule: m.schedule,
            message: m.message,
            target_channel: m.target_channel,
            target_chat_id: m.target_chat_id,
            context: m.context,
            enabled: m.enabled != 0,
            last_run_at: m.last_run_at,
            created_at: m.created_at,
        }
    }
}

// ── Raw read-only pool (for query_readonly) ───────────────────────────────────
//
// SeaORM's QueryResult does not expose column names, so we keep a raw sqlx
// pool specifically for the DB-proxy feature that needs dynamic column
// introspection.

enum RoPool {
    Sqlite(SqlitePool),
    Postgres(PgPool),
}

// ── Placeholder translator (? → $N for Postgres) ─────────────────────────────

/// Translate SQLite-style `?` placeholders to Postgres `$1`, `$2`, … in order.
/// `?` inside single-quoted string literals is left unchanged.
fn translate_placeholders(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len() + 8);
    let mut n: usize = 0;
    let mut in_string = false;
    let mut chars = sql.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\'' if in_string => {
                out.push('\'');
                if chars.peek() == Some(&'\'') {
                    out.push(chars.next().unwrap()); // escaped ''
                } else {
                    in_string = false;
                }
            }
            '\'' => {
                out.push('\'');
                in_string = true;
            }
            '?' if !in_string => {
                n += 1;
                out.push('$');
                out.push_str(&n.to_string());
            }
            other => out.push(other),
        }
    }
    out
}

// ── StateDb ───────────────────────────────────────────────────────────────────

/// Database-backed store for bot sessions, messages, tool calls, and cron jobs.
///
/// Backend is selected at runtime from the connection URL scheme:
/// - `sqlite://…` — SQLite (default when `database_url` is not configured)
/// - `postgres://…` / `postgresql://…` — PostgreSQL
///
/// Entity operations use SeaORM; the read-only proxy query path keeps a raw
/// sqlx pool for dynamic column-name introspection.
#[derive(Clone)]
pub struct StateDb {
    db: DatabaseConnection,
    ro: std::sync::Arc<RoPool>,
}

impl StateDb {
    /// Open the database at `url` and run all pending migrations.
    pub async fn open(url: &str) -> anyhow::Result<Self> {
        // SeaORM connection (rw) — used for all entity CRUD.
        let db = sea_orm::Database::connect(url)
            .await
            .context("opening state db")?;

        migrator::Migrator::up(&db, None)
            .await
            .context("running migrations")?;

        // Raw read-only sqlx pool — used only by query_readonly.
        let ro = if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            let opts = PgConnectOptions::from_str(url)
                .context("parsing postgres url")?
                .options([("default_transaction_read_only", "on")]);
            let pool = PgPool::connect_with(opts)
                .await
                .context("opening ro postgres pool")?;
            RoPool::Postgres(pool)
        } else {
            // sqlite:// or sqlite: prefix
            let path_str = url
                .strip_prefix("sqlite://")
                .or_else(|| url.strip_prefix("sqlite:"))
                .unwrap_or(url);
            let opts = SqliteConnectOptions::new()
                .filename(path_str)
                .read_only(true);
            let pool = SqlitePool::connect_with(opts)
                .await
                .context("opening ro sqlite pool")?;
            RoPool::Sqlite(pool)
        };

        info!("state db opened: {url}");
        Ok(Self { db, ro: std::sync::Arc::new(ro) })
    }

    // ── sessions ──────────────────────────────────────────────────────────────

    pub async fn get_session(&self, id: &str) -> anyhow::Result<Option<SessionRecord>> {
        let record = entity::bot_session::Entity::find_by_id(id)
            .one(&self.db)
            .await?
            .map(SessionRecord::from);
        Ok(record)
    }

    pub async fn upsert_session(&self, s: &SessionRecord) -> anyhow::Result<()> {
        use sea_orm::sea_query::OnConflict;

        let now = now_millis();
        let active = entity::bot_session::ActiveModel {
            id: Set(s.id.clone()),
            channel: Set(s.channel.clone()),
            chat_id: Set(s.chat_id.clone()),
            thread_id: Set(s.thread_id.clone()),
            created_at: Set(s.created_at),
            updated_at: Set(now),
        };

        entity::bot_session::Entity::insert(active)
            .on_conflict(
                OnConflict::column(entity::bot_session::Column::Id)
                    .update_columns([
                        entity::bot_session::Column::ThreadId,
                        entity::bot_session::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(&self.db)
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

    // ── messages ──────────────────────────────────────────────────────────────

    pub async fn save_message(
        &self,
        session_id: &str,
        sender: &str,
        role: &str,
        text: &str,
    ) -> anyhow::Result<()> {
        let active = entity::bot_message::ActiveModel {
            id: sea_orm::ActiveValue::NotSet,
            session_id: Set(session_id.to_string()),
            sender: Set(sender.to_string()),
            role: Set(role.to_string()),
            text: Set(text.to_string()),
            timestamp: Set(now_millis()),
        };
        active.insert(&self.db).await?;
        Ok(())
    }

    // ── cron jobs ─────────────────────────────────────────────────────────────

    pub async fn list_cron_jobs(&self) -> anyhow::Result<Vec<CronJobRecord>> {
        let models = entity::cron_job::Entity::find()
            .order_by(entity::cron_job::Column::CreatedAt, Order::Asc)
            .all(&self.db)
            .await?;
        Ok(models.into_iter().map(CronJobRecord::from).collect())
    }

    pub async fn create_cron_job(&self, job: &CronJobRecord) -> anyhow::Result<()> {
        let active = entity::cron_job::ActiveModel {
            id: Set(job.id.clone()),
            label: Set(job.label.clone()),
            schedule: Set(job.schedule.clone()),
            message: Set(job.message.clone()),
            target_channel: Set(job.target_channel.clone()),
            target_chat_id: Set(job.target_chat_id.clone()),
            context: Set(job.context.clone()),
            enabled: Set(job.enabled as i32),
            last_run_at: Set(job.last_run_at),
            created_at: Set(job.created_at),
        };
        active.insert(&self.db).await?;
        Ok(())
    }

    pub async fn update_cron_job_last_run(&self, id: &str, last_run_at: i64) -> anyhow::Result<()> {
        entity::cron_job::Entity::update_many()
            .col_expr(
                entity::cron_job::Column::LastRunAt,
                sea_orm::sea_query::Expr::value(last_run_at),
            )
            .filter(entity::cron_job::Column::Id.eq(id))
            .exec(&self.db)
            .await?;
        Ok(())
    }

    pub async fn delete_cron_job(&self, id: &str) -> anyhow::Result<bool> {
        let result = entity::cron_job::Entity::delete_by_id(id)
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected > 0)
    }

    // ── tool calls ────────────────────────────────────────────────────────────

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
        let active = entity::bot_tool_call::ActiveModel {
            id: sea_orm::ActiveValue::NotSet,
            session_id: Set(session_id.to_string()),
            tool_call_id: Set(tool_call_id.to_string()),
            tool_name: Set(tool_name.to_string()),
            arguments: Set(arguments.to_string()),
            output: Set(output.to_string()),
            status: Set(status.to_string()),
            duration_ms: Set(duration_ms),
            timestamp: Set(now_millis()),
        };
        active.insert(&self.db).await?;
        Ok(())
    }

    // ── read-only proxy queries ───────────────────────────────────────────────

    /// Execute a read-only SQL query and return results as JSON-compatible rows.
    ///
    /// Only SELECT, WITH, and EXPLAIN statements are allowed (plus PRAGMA on
    /// SQLite). Uses a separate read-only connection pool so the database engine
    /// enforces the restriction independently of the text-level guard.
    ///
    /// Use `?`-style placeholders regardless of backend; they are translated to
    /// `$N` automatically for Postgres.
    pub async fn query_readonly(
        &self,
        sql: &str,
        params: &[String],
    ) -> anyhow::Result<(Vec<String>, Vec<Vec<serde_json::Value>>)> {
        let stripped = sql.trim_start().to_lowercase();

        let is_pragma = stripped.starts_with("pragma");
        let allowed = stripped.starts_with("select")
            || stripped.starts_with("with")
            || stripped.starts_with("explain")
            || (is_pragma && matches!(self.ro.as_ref(), RoPool::Sqlite(_)));

        if !allowed {
            anyhow::bail!("Only SELECT / WITH / EXPLAIN queries are allowed");
        }
        if stripped.contains("attach") {
            anyhow::bail!("ATTACH is not allowed");
        }

        match self.ro.as_ref() {
            RoPool::Sqlite(pool) => {
                let mut query = sqlx::query(sql);
                for p in params {
                    query = query.bind(p);
                }
                let rows = query.fetch_all(pool).await?;
                sqlite_rows_to_json(&rows)
            }
            RoPool::Postgres(pool) => {
                let pg_sql = translate_placeholders(sql);
                let mut query = sqlx::query(&pg_sql);
                for p in params {
                    query = query.bind(p);
                }
                let rows = query.fetch_all(pool).await?;
                postgres_rows_to_json(&rows)
            }
        }
    }
}

// ── row → JSON helpers ────────────────────────────────────────────────────────

fn sqlite_rows_to_json(
    rows: &[sqlx::sqlite::SqliteRow],
) -> anyhow::Result<(Vec<String>, Vec<Vec<serde_json::Value>>)> {
    if rows.is_empty() {
        return Ok((vec![], vec![]));
    }
    let columns: Vec<String> = rows[0].columns().iter().map(|c| c.name().to_string()).collect();
    let result_rows = rows
        .iter()
        .map(|row| {
            (0..columns.len())
                .map(|i| sqlite_cell(row, i))
                .collect()
        })
        .collect();
    Ok((columns, result_rows))
}

fn sqlite_cell(row: &sqlx::sqlite::SqliteRow, i: usize) -> serde_json::Value {
    if let Ok(v) = row.try_get::<i64, _>(i) {
        serde_json::Value::Number(v.into())
    } else if let Ok(v) = row.try_get::<f64, _>(i) {
        serde_json::json!(v)
    } else if let Ok(v) = row.try_get::<String, _>(i) {
        serde_json::Value::String(v)
    } else if let Ok(v) = row.try_get::<Option<String>, _>(i) {
        v.map(serde_json::Value::String)
            .unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::Value::Null
    }
}

fn postgres_rows_to_json(
    rows: &[sqlx::postgres::PgRow],
) -> anyhow::Result<(Vec<String>, Vec<Vec<serde_json::Value>>)> {
    if rows.is_empty() {
        return Ok((vec![], vec![]));
    }
    let columns: Vec<String> = rows[0].columns().iter().map(|c| c.name().to_string()).collect();
    let result_rows = rows
        .iter()
        .map(|row| {
            (0..columns.len())
                .map(|i| postgres_cell(row, i))
                .collect()
        })
        .collect();
    Ok((columns, result_rows))
}

fn postgres_cell(row: &sqlx::postgres::PgRow, i: usize) -> serde_json::Value {
    if let Ok(v) = row.try_get::<i64, _>(i) {
        serde_json::Value::Number(v.into())
    } else if let Ok(v) = row.try_get::<i32, _>(i) {
        serde_json::Value::Number(v.into())
    } else if let Ok(v) = row.try_get::<f64, _>(i) {
        serde_json::json!(v)
    } else if let Ok(v) = row.try_get::<String, _>(i) {
        serde_json::Value::String(v)
    } else if let Ok(v) = row.try_get::<Option<String>, _>(i) {
        v.map(serde_json::Value::String)
            .unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::Value::Null
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::translate_placeholders;

    #[test]
    fn translate_no_placeholders() {
        assert_eq!(
            translate_placeholders("SELECT COUNT(*) as total FROM bot_tool_calls"),
            "SELECT COUNT(*) as total FROM bot_tool_calls"
        );
    }

    #[test]
    fn translate_single() {
        assert_eq!(
            translate_placeholders("SELECT * FROM bot_sessions WHERE id = ?"),
            "SELECT * FROM bot_sessions WHERE id = $1"
        );
    }

    #[test]
    fn translate_multiple() {
        assert_eq!(
            translate_placeholders("UPDATE cron_jobs SET last_run_at = ? WHERE id = ?"),
            "UPDATE cron_jobs SET last_run_at = $1 WHERE id = $2"
        );
    }

    #[test]
    fn translate_preserves_question_in_string_literal() {
        assert_eq!(
            translate_placeholders("SELECT * FROM t WHERE note = 'is it?' AND id = ?"),
            "SELECT * FROM t WHERE note = 'is it?' AND id = $1"
        );
    }

    #[test]
    fn translate_escaped_quote_in_string() {
        assert_eq!(
            translate_placeholders("SELECT * FROM t WHERE x = 'it''s' AND id = ?"),
            "SELECT * FROM t WHERE x = 'it''s' AND id = $1"
        );
    }
}
