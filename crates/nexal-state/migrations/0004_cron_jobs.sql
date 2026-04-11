CREATE TABLE IF NOT EXISTS cron_jobs (
    id               TEXT    PRIMARY KEY,
    label            TEXT    NOT NULL,
    schedule         TEXT    NOT NULL,
    message          TEXT    NOT NULL,
    target_channel   TEXT    NOT NULL,
    target_chat_id   TEXT    NOT NULL,
    context          TEXT    NOT NULL DEFAULT '',
    enabled          INTEGER NOT NULL DEFAULT 1,
    last_run_at      INTEGER,
    created_at       INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_cron_jobs_enabled ON cron_jobs(enabled);
