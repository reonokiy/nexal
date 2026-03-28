//! Multiple conversation contexts — dashboard for monitoring agent activity.
//!
//! Each context is an independent agent with its own state.
//! The ContextManager tracks all contexts and exposes a snapshot
//! for the TUI dashboard view.

use std::collections::HashMap;

use crate::actor::AgentHandle;

/// Unique identifier for a conversation context.
pub type ContextId = u32;

/// Status of an agent context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextStatus {
    /// Idle, waiting for input.
    Idle,
    /// Processing a turn.
    Working,
    /// Waiting for a dependency to complete.
    Waiting { depends_on: Vec<ContextId> },
}

impl std::fmt::Display for ContextStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::Working => write!(f, "working"),
            Self::Waiting { depends_on } => {
                let deps: Vec<String> = depends_on.iter().map(|d| d.to_string()).collect();
                write!(f, "waiting [{}]", deps.join(","))
            }
        }
    }
}

/// Snapshot of a context for display in the dashboard.
#[derive(Debug, Clone)]
pub struct ContextSnapshot {
    pub id: ContextId,
    pub label: String,
    pub status: ContextStatus,
    pub model: String,
    pub activity: String,
    pub is_active: bool,
}

/// Manages multiple conversation contexts.
pub struct ContextManager {
    contexts: HashMap<ContextId, ContextEntry>,
    active: ContextId,
    next_id: ContextId,
}

struct ContextEntry {
    handle: AgentHandle,
    label: String,
    model: String,
    status: ContextStatus,
    activity: String,
}

impl ContextManager {
    pub fn new(initial_handle: AgentHandle, label: String, model: String) -> Self {
        let mut contexts = HashMap::new();
        contexts.insert(
            1,
            ContextEntry {
                handle: initial_handle,
                label,
                model,
                status: ContextStatus::Idle,
                activity: String::new(),
            },
        );
        Self {
            contexts,
            active: 1,
            next_id: 2,
        }
    }

    /// Get the active context's handle.
    pub fn active_handle(&self) -> Option<&AgentHandle> {
        self.contexts.get(&self.active).map(|e| &e.handle)
    }

    pub fn active_id(&self) -> ContextId {
        self.active
    }

    /// Add a new context, returns its ID.
    pub fn add(
        &mut self,
        handle: AgentHandle,
        label: String,
        model: String,
    ) -> ContextId {
        let id = self.next_id;
        self.next_id += 1;
        self.contexts.insert(
            id,
            ContextEntry {
                handle,
                label,
                model,
                status: ContextStatus::Idle,
                activity: String::new(),
            },
        );
        id
    }

    /// Switch active context.
    pub fn switch(&mut self, id: ContextId) -> bool {
        if self.contexts.contains_key(&id) {
            self.active = id;
            true
        } else {
            false
        }
    }

    /// Close a context. Cannot close the last one.
    pub fn close(&mut self, id: ContextId) -> bool {
        if self.contexts.len() <= 1 || !self.contexts.contains_key(&id) {
            return false;
        }
        self.contexts.remove(&id);
        if self.active == id {
            self.active = *self.contexts.keys().next().unwrap();
        }
        true
    }

    /// Update a context's status and activity text.
    pub fn set_status(&mut self, id: ContextId, status: ContextStatus, activity: String) {
        if let Some(entry) = self.contexts.get_mut(&id) {
            entry.status = status;
            entry.activity = activity;
        }
    }

    /// Get a snapshot of all contexts for the dashboard.
    pub fn snapshot(&self) -> Vec<ContextSnapshot> {
        let mut entries: Vec<_> = self
            .contexts
            .iter()
            .map(|(&id, entry)| ContextSnapshot {
                id,
                label: entry.label.clone(),
                status: entry.status.clone(),
                model: entry.model.clone(),
                activity: entry.activity.clone(),
                is_active: id == self.active,
            })
            .collect();
        entries.sort_by_key(|e| e.id);
        entries
    }

    pub fn len(&self) -> usize {
        self.contexts.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    fn dummy_handle() -> AgentHandle {
        let (tx, _rx) = mpsc::channel(1);
        AgentHandle::new_from_sender(tx)
    }

    #[test]
    fn basic_lifecycle() {
        let mut mgr = ContextManager::new(dummy_handle(), "orchestrator".into(), "kimi".into());
        assert_eq!(mgr.active_id(), 1);
        assert_eq!(mgr.len(), 1);

        let id2 = mgr.add(dummy_handle(), "research".into(), "kimi".into());
        assert_eq!(id2, 2);
        assert_eq!(mgr.len(), 2);

        assert!(mgr.switch(2));
        assert_eq!(mgr.active_id(), 2);

        assert!(mgr.close(2));
        assert_eq!(mgr.active_id(), 1);
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn cannot_close_last() {
        let mut mgr = ContextManager::new(dummy_handle(), "only".into(), "kimi".into());
        assert!(!mgr.close(1));
    }

    #[test]
    fn snapshot_shows_status() {
        let mut mgr = ContextManager::new(dummy_handle(), "orchestrator".into(), "kimi-k2.5".into());
        mgr.add(dummy_handle(), "research".into(), "kimi-k2.5".into());

        mgr.set_status(1, ContextStatus::Idle, String::new());
        mgr.set_status(2, ContextStatus::Working, "analyzing code...".into());

        let snap = mgr.snapshot();
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].label, "orchestrator");
        assert_eq!(snap[0].status, ContextStatus::Idle);
        assert!(snap[0].is_active);
        assert_eq!(snap[1].label, "research");
        assert_eq!(snap[1].status, ContextStatus::Working);
        assert_eq!(snap[1].activity, "analyzing code...");
    }
}
