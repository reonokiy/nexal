//! Multiple conversation contexts per user session.
//!
//! Each context is an independent `AgentHandle` with its own thread_id
//! and conversation history. Users can create, switch, and close contexts.

use std::collections::HashMap;

use crate::actor::AgentHandle;

/// Unique identifier for a conversation context.
pub type ContextId = u32;

/// Manages multiple conversation contexts within a single user session.
pub struct ContextManager {
    contexts: HashMap<ContextId, ContextEntry>,
    active: ContextId,
    next_id: ContextId,
}

struct ContextEntry {
    handle: AgentHandle,
    label: String,
}

impl ContextManager {
    /// Create a new manager with a single initial context.
    pub fn new(initial_handle: AgentHandle, label: String) -> Self {
        let mut contexts = HashMap::new();
        contexts.insert(1, ContextEntry {
            handle: initial_handle,
            label,
        });
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

    /// Get the active context ID.
    pub fn active_id(&self) -> ContextId {
        self.active
    }

    /// Add a new context, returns its ID.
    pub fn add(&mut self, handle: AgentHandle, label: String) -> ContextId {
        let id = self.next_id;
        self.next_id += 1;
        self.contexts.insert(id, ContextEntry { handle, label });
        id
    }

    /// Switch to a different context.
    pub fn switch(&mut self, id: ContextId) -> bool {
        if self.contexts.contains_key(&id) {
            self.active = id;
            true
        } else {
            false
        }
    }

    /// Close a context. Cannot close the last remaining context.
    pub fn close(&mut self, id: ContextId) -> bool {
        if self.contexts.len() <= 1 || !self.contexts.contains_key(&id) {
            return false;
        }
        self.contexts.remove(&id);
        if self.active == id {
            // Switch to the first available context
            self.active = *self.contexts.keys().next().unwrap();
        }
        true
    }

    /// List all contexts: (id, label, is_active).
    pub fn list(&self) -> Vec<(ContextId, String, bool)> {
        let mut entries: Vec<_> = self
            .contexts
            .iter()
            .map(|(&id, entry)| (id, entry.label.clone(), id == self.active))
            .collect();
        entries.sort_by_key(|(id, _, _)| *id);
        entries
    }

    /// Number of contexts.
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
        let mut mgr = ContextManager::new(dummy_handle(), "default".into());
        assert_eq!(mgr.active_id(), 1);
        assert_eq!(mgr.len(), 1);

        let id2 = mgr.add(dummy_handle(), "task-2".into());
        assert_eq!(id2, 2);
        assert_eq!(mgr.len(), 2);

        assert!(mgr.switch(2));
        assert_eq!(mgr.active_id(), 2);

        // Close context 2 — should succeed (context 1 still exists)
        assert!(mgr.close(2));
        assert_eq!(mgr.active_id(), 1); // auto-switched to remaining
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn cannot_close_last() {
        let mut mgr = ContextManager::new(dummy_handle(), "only".into());
        assert!(!mgr.close(1)); // can't close the only context
    }

    #[test]
    fn list_contexts() {
        let mut mgr = ContextManager::new(dummy_handle(), "ctx-1".into());
        mgr.add(dummy_handle(), "ctx-2".into());
        let list = mgr.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], (1, "ctx-1".into(), true));
        assert_eq!(list[1], (2, "ctx-2".into(), false));
    }
}
