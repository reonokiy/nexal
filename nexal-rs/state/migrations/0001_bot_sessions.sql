CREATE TABLE IF NOT EXISTS bot_sessions (
    id                   TEXT PRIMARY KEY,  -- "channel:chat_id"
    channel              TEXT NOT NULL,     -- "telegram" | "discord" | "cli"
    chat_id              TEXT NOT NULL,
    thread_id            TEXT,              -- codex thread ID for resume
    created_at           INTEGER NOT NULL,  -- unix millis
    updated_at           INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_bot_sessions_channel_chat
    ON bot_sessions(channel, chat_id);
