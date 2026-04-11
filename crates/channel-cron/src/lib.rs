//! Cron channel — agent-scheduled wake-ups.
//!
//! The agent can create, list, and delete cron jobs via skill scripts.
//! Jobs are persisted to the StateDb (SQLite) and survive restarts.
//! When a job fires, it injects a message into the target session
//! (e.g. `telegram:-12345`) so the agent can act with context.
//!
//! ## Job types
//!
//! - **Cron expression**: standard 5-field crontab (`0 */2 * * *`)
//! - **Interval**: fire every N seconds (`every:300` = every 5 min)
//! - **One-shot**: fire once at a specific time (`once:2026-04-02T18:00:00`)

use std::sync::Arc;

use chrono::{DateTime, Utc};
use nexal_channel_core::{Channel, IncomingMessage, MessageCallback};
use nexal_config::NexalConfig;
use nexal_state::StateDb;
use tracing::{debug, info, warn};

/// Default tick interval: 15 seconds.
const DEFAULT_TICK_SECS: u64 = 15;

pub struct CronChannel {
    config: Arc<NexalConfig>,
    db: Arc<StateDb>,
}

impl CronChannel {
    pub fn new(config: Arc<NexalConfig>, db: Arc<StateDb>) -> Self {
        Self { config, db }
    }

    fn tick_interval(&self) -> std::time::Duration {
        let secs = self.config.channel.cron.tick_interval_secs.unwrap_or(DEFAULT_TICK_SECS);
        std::time::Duration::from_secs(secs)
    }
}

#[async_trait::async_trait]
impl Channel for CronChannel {
    fn name(&self) -> &str {
        "cron"
    }

    async fn start(&self, on_message: MessageCallback) -> anyhow::Result<()> {
        let tick = self.tick_interval();
        info!(tick_secs = tick.as_secs(), "cron channel started");

        let mut ticker = tokio::time::interval(tick);

        loop {
            ticker.tick().await;

            let jobs = match self.db.list_cron_jobs().await {
                Ok(j) => j,
                Err(e) => {
                    warn!("failed to load cron jobs: {e}");
                    continue;
                }
            };

            let now = Utc::now();
            let now_millis = now.timestamp_millis();

            for job in &jobs {
                if !job.enabled {
                    continue;
                }
                if should_fire(job, now) {
                    info!(job_id = %job.id, label = %job.label, "cron job firing");

                    let mut text = format!("[cron:{}] {}", job.label, job.message);
                    if !job.context.is_empty() {
                        text.push_str(&format!(
                            "\n\nContext from when this was scheduled:\n{}",
                            job.context
                        ));
                    }

                    let msg = IncomingMessage::new(
                        &job.target_channel,
                        &job.target_chat_id,
                        "cron",
                        &text,
                    );

                    on_message(msg);

                    if let Err(e) = self.db.update_cron_job_last_run(&job.id, now_millis).await {
                        warn!(job_id = %job.id, "failed to update last_run: {e}");
                    }

                    // Remove one-shot jobs after firing.
                    if job.schedule.starts_with("once:") {
                        if let Err(e) = self.db.delete_cron_job(&job.id).await {
                            warn!(job_id = %job.id, "failed to delete one-shot job: {e}");
                        }
                    }
                }
            }
        }
    }

    async fn send(&self, _chat_id: &str, _text: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Check if a job should fire right now.
fn should_fire(job: &nexal_state::CronJobRecord, now: DateTime<Utc>) -> bool {
    let last_run: Option<DateTime<Utc>> = job
        .last_run_at
        .and_then(|ms| DateTime::from_timestamp_millis(ms));

    if job.schedule.starts_with("every:") {
        let secs: u64 = job.schedule["every:".len()..].parse().unwrap_or(0);
        if secs == 0 {
            return false;
        }
        match last_run {
            Some(last) => (now - last).num_seconds() >= secs as i64,
            None => true,
        }
    } else if job.schedule.starts_with("once:") {
        let target: Option<DateTime<Utc>> = job.schedule["once:".len()..]
            .parse()
            .ok();
        match (target, last_run) {
            (Some(t), None) => now >= t,
            _ => false,
        }
    } else {
        fire_cron_expression(&job.schedule, now, last_run)
    }
}

fn fire_cron_expression(
    expr: &str,
    now: DateTime<Utc>,
    last_run: Option<DateTime<Utc>>,
) -> bool {
    use std::str::FromStr;

    let schedule = match cron::Schedule::from_str(expr) {
        Ok(s) => s,
        Err(e) => {
            debug!("invalid cron expression '{expr}': {e}");
            return false;
        }
    };

    let reference = last_run.unwrap_or_else(|| now - chrono::Duration::hours(24));

    let mut upcoming = schedule.after(&reference);
    match upcoming.next() {
        Some(next_fire) => next_fire <= now,
        None => false,
    }
}
