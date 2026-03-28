use super::ChatWidget;
use crate::app_event::AppEvent;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;
use crate::history_cell;

impl ChatWidget {
    pub(crate) fn open_context_menu(&mut self) {
        let items = vec![
            SelectionItem {
                name: "New context".to_string(),
                description: Some("Start a fresh conversation context".to_string()),
                actions: vec![Box::new(|tx| {
                    tx.send(AppEvent::InsertHistoryCell(Box::new(
                        history_cell::new_info_event(
                            "Context management is available in multi-context mode (coming soon)".to_string(),
                            Some("Each context will have independent history and agent state".to_string()),
                        ),
                    )));
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "List contexts".to_string(),
                description: Some("Show all active conversation contexts".to_string()),
                actions: vec![Box::new(|tx| {
                    tx.send(AppEvent::InsertHistoryCell(Box::new(
                        history_cell::new_info_event(
                            "[1] default (active)".to_string(),
                            None,
                        ),
                    )));
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
        ];

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Contexts".to_string()),
            subtitle: Some("Manage conversation contexts".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
    }
}
