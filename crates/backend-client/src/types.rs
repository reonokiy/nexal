use serde::Deserialize;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Stub types that replace the former nexal-backend-openapi-models re-exports.
// These are kept structurally compatible so that downstream code compiles, but
// the Client methods that would return them now always error out.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Deserialize)]
pub struct PaginatedListTaskListItem {
    #[serde(default)]
    pub items: Vec<TaskListItem>,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct TaskListItem {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub updated_at: Option<f64>,
    #[serde(default)]
    pub task_status_display: Option<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    pub pull_requests: Option<Vec<serde_json::Value>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ConfigFileResponse {
    #[serde(default)]
    pub contents: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
}

// ---------------------------------------------------------------------------
// Task-details types (formerly hand-rolled in this crate).
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Deserialize)]
pub struct CodeTaskDetailsResponse {
    #[serde(default)]
    pub current_user_turn: Option<Turn>,
    #[serde(default)]
    pub current_assistant_turn: Option<Turn>,
    #[serde(default)]
    pub current_diff_task_turn: Option<Turn>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Turn {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub attempt_placement: Option<i64>,
    #[serde(default, rename = "turn_status")]
    pub turn_status: Option<String>,
    #[serde(default)]
    pub sibling_turn_ids: Vec<String>,
    #[serde(default)]
    pub input_items: Vec<serde_json::Value>,
    #[serde(default)]
    pub output_items: Vec<serde_json::Value>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct TurnAttemptsSiblingTurnsResponse {
    #[serde(default)]
    pub sibling_turns: Vec<HashMap<String, serde_json::Value>>,
}

// ---------------------------------------------------------------------------
// Extension trait — all methods return empty/None since the backend is gone.
// ---------------------------------------------------------------------------

pub trait CodeTaskDetailsResponseExt {
    fn unified_diff(&self) -> Option<String>;
    fn assistant_text_messages(&self) -> Vec<String>;
    fn user_text_prompt(&self) -> Option<String>;
    fn assistant_error_message(&self) -> Option<String>;
}

impl CodeTaskDetailsResponseExt for CodeTaskDetailsResponse {
    fn unified_diff(&self) -> Option<String> {
        None
    }

    fn assistant_text_messages(&self) -> Vec<String> {
        Vec::new()
    }

    fn user_text_prompt(&self) -> Option<String> {
        None
    }

    fn assistant_error_message(&self) -> Option<String> {
        None
    }
}
