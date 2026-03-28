use super::ChatWidget;
use crate::app_event::AppEvent;
use crate::history_cell;

impl ChatWidget {
    /// Show a dashboard of all agent contexts and their current status.
    pub(crate) fn open_context_menu(&mut self) {
        // For now, show a static dashboard view.
        // When ContextManager is wired in, this will show live agent status.
        let mut lines = Vec::new();
        lines.push("Contexts:".to_string());
        lines.push(format!(
            "  [1] orchestrator  idle  {}",
            std::env::var("LLM_MODEL").unwrap_or_else(|_| "default".into())
        ));

        let msg = lines.join("\n");
        self.app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
            history_cell::new_info_event(msg, None),
        )));
    }
}
