CREATE TABLE IF NOT EXISTS bot_messages (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id  TEXT    NOT NULL REFERENCES bot_sessions(id),
    sender      TEXT    NOT NULL,
    role        TEXT    NOT NULL CHECK(role IN ('user', 'assistant', 'system')),
    text        TEXT    NOT NULL,
    timestamp   INTEGER NOT NULL   -- unix millis
);

CREATE INDEX IF NOT EXISTS idx_bot_messages_session
    ON bot_messages(session_id, timestamp);
