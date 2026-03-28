CREATE TABLE IF NOT EXISTS bot_tool_calls (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id      TEXT    NOT NULL REFERENCES bot_sessions(id),
    tool_call_id    TEXT    NOT NULL,
    tool_name       TEXT    NOT NULL,
    arguments       TEXT    NOT NULL DEFAULT '{}',
    output          TEXT    NOT NULL DEFAULT '',
    status          TEXT    NOT NULL DEFAULT 'ok',
    duration_ms     INTEGER,
    timestamp       INTEGER NOT NULL  -- unix millis
);

CREATE INDEX IF NOT EXISTS idx_bot_tool_calls_session
    ON bot_tool_calls(session_id, timestamp);
