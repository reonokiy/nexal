//! Sync TUI session events to StateDb.
//!
//! The TUI's core writes conversation history to rollout JSONL files.
//! This module watches for new rollout files and syncs user messages
//! and assistant responses to nexal.db so that chatlog/toollog skills
//! can query them.

use std::path::Path;
use std::sync::Arc;

use nexal_state::StateDb;
use tokio::time::{Duration, interval};


/// Start a background task that periodically syncs the latest conversation
/// state to StateDb. Reads from the rollout session directory.
pub fn start_sync(db: Arc<StateDb>, nexal_home: &Path) -> tokio::task::JoinHandle<()> {
    let sessions_dir = nexal_home.join("sessions");
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(2));
        let mut last_size: u64 = 0;

        loop {
            tick.tick().await;

            // Find the latest session file
            let latest = match find_latest_session(&sessions_dir).await {
                Some(p) => p,
                None => continue,
            };

            // Check if file grew
            let meta = match tokio::fs::metadata(&latest).await {
                Ok(m) => m,
                Err(_) => continue,
            };
            let size = meta.len();
            if size == last_size {
                continue;
            }
            last_size = size;

            // Read and parse new lines
            if let Ok(content) = tokio::fs::read_to_string(&latest).await {
                sync_jsonl_to_db(&db, &content).await;
            }
        }
    })
}

/// Find the most recently modified .jsonl file in the sessions directory.
async fn find_latest_session(sessions_dir: &Path) -> Option<std::path::PathBuf> {
    let mut latest: Option<(std::path::PathBuf, std::time::SystemTime)> = None;

    // Sessions are organized as sessions/<date>/<name>.jsonl
    let mut date_dirs = match tokio::fs::read_dir(sessions_dir).await {
        Ok(d) => d,
        Err(_) => return None,
    };

    while let Ok(Some(date_entry)) = date_dirs.next_entry().await {
        if !date_entry.path().is_dir() {
            continue;
        }
        let mut files = match tokio::fs::read_dir(date_entry.path()).await {
            Ok(f) => f,
            Err(_) => continue,
        };
        while let Ok(Some(file_entry)) = files.next_entry().await {
            let path = file_entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if let Ok(meta) = file_entry.metadata().await {
                if let Ok(modified) = meta.modified() {
                    if latest.as_ref().map_or(true, |(_, t)| modified > *t) {
                        latest = Some((path, modified));
                    }
                }
            }
        }
    }

    latest.map(|(p, _)| p)
}

/// Parse JSONL lines and write messages/tool calls to StateDb.
async fn sync_jsonl_to_db(db: &StateDb, content: &str) {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let event: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match event_type {
            "message" | "agent_message" => {
                let role = event
                    .get("role")
                    .and_then(|v| v.as_str())
                    .unwrap_or("assistant");
                let text = event
                    .get("text")
                    .or_else(|| event.get("content"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !text.is_empty() {
                    
                    let session = match db.get_or_create_session("tui", "local").await {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    let sender = if role == "user" { "user" } else { "nexal" };
                    let _ = db.save_message(&session.id, sender, role, text).await;
                }
            }
            _ => {
                // Skip other event types for now
            }
        }
    }
}
