use super::ChatWidget;
use crate::app_event::AppEvent;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;
use crate::history_cell;

impl ChatWidget {
    pub(crate) fn open_channels_menu(&mut self) {
        let telegram_active = std::env::var("TELEGRAM_BOT_TOKEN").is_ok();
        let discord_active = std::env::var("DISCORD_BOT_TOKEN").is_ok();

        let telegram_status = if telegram_active { "active" } else { "not configured" };
        let discord_status = if discord_active { "active" } else { "not configured" };

        let items = vec![
            SelectionItem {
                name: format!("Telegram  [{telegram_status}]"),
                description: Some(if telegram_active {
                    "Listening for messages. Set TELEGRAM_BOT_TOKEN to configure.".to_string()
                } else {
                    "Set TELEGRAM_BOT_TOKEN in .env to enable.".to_string()
                }),
                actions: vec![Box::new(move |tx| {
                    let msg = if telegram_active {
                        "Telegram channel is active."
                    } else {
                        "Set TELEGRAM_BOT_TOKEN in .env and restart to enable Telegram."
                    };
                    tx.send(AppEvent::InsertHistoryCell(Box::new(
                        history_cell::new_info_event(msg.to_string(), None),
                    )));
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: format!("Discord   [{discord_status}]"),
                description: Some(if discord_active {
                    "Listening for messages. Set DISCORD_BOT_TOKEN to configure.".to_string()
                } else {
                    "Set DISCORD_BOT_TOKEN in .env to enable.".to_string()
                }),
                actions: vec![Box::new(move |tx| {
                    let msg = if discord_active {
                        "Discord channel is active."
                    } else {
                        "Set DISCORD_BOT_TOKEN in .env and restart to enable Discord."
                    };
                    tx.send(AppEvent::InsertHistoryCell(Box::new(
                        history_cell::new_info_event(msg.to_string(), None),
                    )));
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
        ];

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Channels".to_string()),
            subtitle: Some("Manage active channels".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
    }
}
