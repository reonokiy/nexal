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
///
/// Sessions are organized as `sessions/<date>/<name>.jsonl`. We look one
/// level deep into each date subdirectory and return the newest jsonl file
/// across all of them.
async fn find_latest_session(sessions_dir: &Path) -> Option<std::path::PathBuf> {
    let mut date_dirs = tokio::fs::read_dir(sessions_dir).await.ok()?;
    let mut latest: Option<(std::path::PathBuf, std::time::SystemTime)> = None;

    while let Ok(Some(date_entry)) = date_dirs.next_entry().await {
        if !date_entry.path().is_dir() {
            continue;
        }
        let Some((path, mtime)) = latest_jsonl_in(&date_entry.path()).await else {
            continue;
        };
        if latest.as_ref().is_none_or(|(_, t)| mtime > *t) {
            latest = Some((path, mtime));
        }
    }

    latest.map(|(p, _)| p)
}

/// Return the most recently modified `.jsonl` in `dir` alongside its mtime.
/// Returns `None` if the directory cannot be read or contains no jsonl files.
async fn latest_jsonl_in(
    dir: &Path,
) -> Option<(std::path::PathBuf, std::time::SystemTime)> {
    let mut entries = tokio::fs::read_dir(dir).await.ok()?;
    let mut latest: Option<(std::path::PathBuf, std::time::SystemTime)> = None;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(meta) = entry.metadata().await else { continue };
        let Ok(mtime) = meta.modified() else { continue };
        if latest.as_ref().is_none_or(|(_, t)| mtime > *t) {
            latest = Some((path, mtime));
        }
    }
    latest
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
