#![allow(clippy::unwrap_used)]

use nexal_core::AuthManager;
use nexal_core::auth::AuthCredentialsStoreMode;
use nexal_core::auth::login_with_api_key;
use nexal_core::auth::read_openai_api_key_from_env;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::prelude::Widget;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::BorderType;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::WidgetRef;
use ratatui::widgets::Wrap;

use nexal_core::auth::AuthMode;
use nexal_protocol::config_types::ForcedLoginMethod;
use std::sync::RwLock;

use crate::LoginStatus;
use crate::onboarding::onboarding_screen::KeyboardHandler;
use crate::onboarding::onboarding_screen::StepStateProvider;
use crate::tui::FrameRequester;

/// Marks buffer cells that have cyan+underlined style as an OSC 8 hyperlink.
///
/// Terminal emulators recognise the OSC 8 escape sequence and treat the entire
/// marked region as a single clickable link, regardless of row wrapping.  This
/// is necessary because ratatui's cell-based rendering emits `MoveTo` at every
/// row boundary, which breaks normal terminal URL detection for long URLs that
/// wrap across multiple rows.
pub(crate) fn mark_url_hyperlink(buf: &mut Buffer, area: Rect, url: &str) {
    // Sanitize: strip any characters that could break out of the OSC 8
    // sequence (ESC or BEL) to prevent terminal escape injection from a
    // malformed or compromised upstream URL.
    let safe_url: String = url
        .chars()
        .filter(|&c| c != '\x1B' && c != '\x07')
        .collect();
    if safe_url.is_empty() {
        return;
    }

    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &mut buf[(x, y)];
            // Only mark cells that carry the URL's distinctive style.
            if cell.fg != Color::Cyan || !cell.modifier.contains(Modifier::UNDERLINED) {
                continue;
            }
            let sym = cell.symbol().to_string();
            if sym.trim().is_empty() {
                continue;
            }
            cell.set_symbol(&format!("\x1B]8;;{safe_url}\x07{sym}\x1B]8;;\x07"));
        }
    }
}
use std::path::PathBuf;
use std::sync::Arc;
use super::onboarding_screen::StepState;

#[derive(Clone)]
pub(crate) enum SignInState {
    PickMode,
    ApiKeyEntry(ApiKeyInputState),
    ApiKeyConfigured,
}


const API_KEY_DISABLED_MESSAGE: &str = "API key login is disabled.";

#[derive(Clone, Default)]
pub(crate) struct ApiKeyInputState {
    value: String,
    prepopulated_from_env: bool,
}

impl KeyboardHandler for AuthModeWidget {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if self.handle_api_key_entry_key_event(&key_event) {
            return;
        }

        if key_event.code == KeyCode::Enter {
            let sign_in_state = { (*self.sign_in_state.read().unwrap()).clone() };
            if matches!(sign_in_state, SignInState::PickMode) {
                self.start_api_key_entry();
            }
        }
    }

    fn handle_paste(&mut self, pasted: String) {
        let _ = self.handle_api_key_entry_paste(pasted);
    }
}

#[derive(Clone)]
pub(crate) struct AuthModeWidget {
    pub request_frame: FrameRequester,
    pub error: Option<String>,
    pub sign_in_state: Arc<RwLock<SignInState>>,
    pub nexal_home: PathBuf,
    pub cli_auth_credentials_store_mode: AuthCredentialsStoreMode,
    pub login_status: LoginStatus,
    pub auth_manager: Arc<AuthManager>,
    pub forced_login_method: Option<ForcedLoginMethod>,
}

impl AuthModeWidget {
    fn is_api_login_allowed(&self) -> bool {
        !matches!(self.forced_login_method, Some(ForcedLoginMethod::Chatgpt))
    }

    fn render_pick_mode(&self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line> = vec![
            "  Configure your API key to get started".into(),
            "".into(),
            "> 1. Provide your own API key".cyan().into(),
            "     Pay for what you use"
                .fg(Color::Cyan)
                .add_modifier(Modifier::DIM)
                .into(),
            "".into(),
        ];
        if !self.is_api_login_allowed() {
            lines.push(
                "  API key login is disabled by this workspace."
                    .dim()
                    .into(),
            );
            lines.push("".into());
        }
        lines.push("  Press Enter to continue".dim().into());
        if let Some(err) = &self.error {
            lines.push("".into());
            lines.push(err.as_str().red().into());
        }

        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }

    fn render_api_key_configured(&self, area: Rect, buf: &mut Buffer) {
        let lines = vec![
            "✓ API key configured".fg(Color::Green).into(),
            "".into(),
            "  Nexal will use usage-based billing with your API key.".into(),
        ];

        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }

    fn render_api_key_entry(&self, area: Rect, buf: &mut Buffer, state: &ApiKeyInputState) {
        let [intro_area, input_area, footer_area] = Layout::vertical([
            Constraint::Min(4),
            Constraint::Length(3),
            Constraint::Min(2),
        ])
        .areas(area);

        let mut intro_lines: Vec<Line> = vec![
            Line::from(vec![
                "> ".into(),
                "Use your own API key".bold(),
            ]),
            "".into(),
            "  Paste or type your API key below. It will be stored locally in auth.json.".into(),
            "".into(),
        ];
        if state.prepopulated_from_env {
            intro_lines.push("  Detected OPENAI_API_KEY environment variable.".into());
            intro_lines.push(
                "  Paste a different key if you prefer to use another account."
                    .dim()
                    .into(),
            );
            intro_lines.push("".into());
        }
        Paragraph::new(intro_lines)
            .wrap(Wrap { trim: false })
            .render(intro_area, buf);

        let content_line: Line = if state.value.is_empty() {
            vec!["Paste or type your API key".dim()].into()
        } else {
            Line::from(state.value.clone())
        };
        Paragraph::new(content_line)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .title("API key")
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .render(input_area, buf);

        let mut footer_lines: Vec<Line> = vec![
            "  Press Enter to save".dim().into(),
            "  Press Esc to go back".dim().into(),
        ];
        if let Some(error) = &self.error {
            footer_lines.push("".into());
            footer_lines.push(error.as_str().red().into());
        }
        Paragraph::new(footer_lines)
            .wrap(Wrap { trim: false })
            .render(footer_area, buf);
    }

    fn handle_api_key_entry_key_event(&mut self, key_event: &KeyEvent) -> bool {
        let mut should_save: Option<String> = None;
        let mut should_request_frame = false;

        {
            let mut guard = self.sign_in_state.write().unwrap();
            if let SignInState::ApiKeyEntry(state) = &mut *guard {
                match key_event.code {
                    KeyCode::Esc => {
                        *guard = SignInState::PickMode;
                        self.error = None;
                        should_request_frame = true;
                    }
                    KeyCode::Enter => {
                        let trimmed = state.value.trim().to_string();
                        if trimmed.is_empty() {
                            self.error = Some("API key cannot be empty".to_string());
                            should_request_frame = true;
                        } else {
                            should_save = Some(trimmed);
                        }
                    }
                    KeyCode::Backspace => {
                        if state.prepopulated_from_env {
                            state.value.clear();
                            state.prepopulated_from_env = false;
                        } else {
                            state.value.pop();
                        }
                        self.error = None;
                        should_request_frame = true;
                    }
                    KeyCode::Char(c)
                        if key_event.kind == KeyEventKind::Press
                            && !key_event.modifiers.contains(KeyModifiers::SUPER)
                            && !key_event.modifiers.contains(KeyModifiers::CONTROL)
                            && !key_event.modifiers.contains(KeyModifiers::ALT) =>
                    {
                        if state.prepopulated_from_env {
                            state.value.clear();
                            state.prepopulated_from_env = false;
                        }
                        state.value.push(c);
                        self.error = None;
                        should_request_frame = true;
                    }
                    _ => {}
                }
                // handled; let guard drop before potential save
            } else {
                return false;
            }
        }

        if let Some(api_key) = should_save {
            self.save_api_key(api_key);
        } else if should_request_frame {
            self.request_frame.schedule_frame();
        }
        true
    }

    fn handle_api_key_entry_paste(&mut self, pasted: String) -> bool {
        let trimmed = pasted.trim();
        if trimmed.is_empty() {
            return false;
        }

        let mut guard = self.sign_in_state.write().unwrap();
        if let SignInState::ApiKeyEntry(state) = &mut *guard {
            if state.prepopulated_from_env {
                state.value = trimmed.to_string();
                state.prepopulated_from_env = false;
            } else {
                state.value.push_str(trimmed);
            }
            self.error = None;
        } else {
            return false;
        }

        drop(guard);
        self.request_frame.schedule_frame();
        true
    }

    fn start_api_key_entry(&mut self) {
        if !self.is_api_login_allowed() {
            self.error = Some(API_KEY_DISABLED_MESSAGE.to_string());
            *self.sign_in_state.write().unwrap() = SignInState::PickMode;
            self.request_frame.schedule_frame();
            return;
        }
        self.error = None;
        let prefill_from_env = read_openai_api_key_from_env();
        let mut guard = self.sign_in_state.write().unwrap();
        match &mut *guard {
            SignInState::ApiKeyEntry(state) => {
                if state.value.is_empty() {
                    if let Some(prefill) = prefill_from_env {
                        state.value = prefill;
                        state.prepopulated_from_env = true;
                    } else {
                        state.prepopulated_from_env = false;
                    }
                }
            }
            _ => {
                *guard = SignInState::ApiKeyEntry(ApiKeyInputState {
                    value: prefill_from_env.clone().unwrap_or_default(),
                    prepopulated_from_env: prefill_from_env.is_some(),
                });
            }
        }
        drop(guard);
        self.request_frame.schedule_frame();
    }

    fn save_api_key(&mut self, api_key: String) {
        if !self.is_api_login_allowed() {
            self.error = Some(API_KEY_DISABLED_MESSAGE.to_string());
            *self.sign_in_state.write().unwrap() = SignInState::PickMode;
            self.request_frame.schedule_frame();
            return;
        }
        match login_with_api_key(
            &self.nexal_home,
            &api_key,
            self.cli_auth_credentials_store_mode,
        ) {
            Ok(()) => {
                self.error = None;
                self.login_status = LoginStatus::AuthMode(AuthMode::ApiKey);
                self.auth_manager.reload();
                *self.sign_in_state.write().unwrap() = SignInState::ApiKeyConfigured;
            }
            Err(err) => {
                self.error = Some(format!("Failed to save API key: {err}"));
                let mut guard = self.sign_in_state.write().unwrap();
                if let SignInState::ApiKeyEntry(existing) = &mut *guard {
                    if existing.value.is_empty() {
                        existing.value.push_str(&api_key);
                    }
                    existing.prepopulated_from_env = false;
                } else {
                    *guard = SignInState::ApiKeyEntry(ApiKeyInputState {
                        value: api_key,
                        prepopulated_from_env: false,
                    });
                }
            }
        }

        self.request_frame.schedule_frame();
    }

}

impl StepStateProvider for AuthModeWidget {
    fn get_step_state(&self) -> StepState {
        let sign_in_state = self.sign_in_state.read().unwrap();
        match &*sign_in_state {
            SignInState::PickMode | SignInState::ApiKeyEntry(_) => StepState::InProgress,
            SignInState::ApiKeyConfigured => StepState::Complete,
        }
    }
}

impl WidgetRef for AuthModeWidget {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let sign_in_state = self.sign_in_state.read().unwrap();
        match &*sign_in_state {
            SignInState::PickMode => {
                self.render_pick_mode(area, buf);
            }
            SignInState::ApiKeyEntry(state) => {
                self.render_api_key_entry(area, buf, state);
            }
            SignInState::ApiKeyConfigured => {
                self.render_api_key_configured(area, buf);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    use nexal_core::auth::AuthCredentialsStoreMode;

    fn widget_forced_chatgpt() -> (AuthModeWidget, TempDir) {
        let nexal_home = TempDir::new().unwrap();
        let nexal_home_path = nexal_home.path().to_path_buf();
        let widget = AuthModeWidget {
            request_frame: FrameRequester::test_dummy(),
            error: None,
            sign_in_state: Arc::new(RwLock::new(SignInState::PickMode)),
            nexal_home: nexal_home_path.clone(),
            cli_auth_credentials_store_mode: AuthCredentialsStoreMode::File,
            login_status: LoginStatus::NotAuthenticated,
            auth_manager: AuthManager::shared(
                nexal_home_path,
                false,
                AuthCredentialsStoreMode::File,
            ),
            forced_login_method: Some(ForcedLoginMethod::Chatgpt),
        };
        (widget, nexal_home)
    }

    #[test]
    fn api_key_flow_disabled_when_chatgpt_forced() {
        let (mut widget, _tmp) = widget_forced_chatgpt();

        widget.start_api_key_entry();

        assert_eq!(widget.error.as_deref(), Some(API_KEY_DISABLED_MESSAGE));
        assert!(matches!(
            &*widget.sign_in_state.read().unwrap(),
            SignInState::PickMode
        ));
    }

    #[test]
    fn saving_api_key_is_blocked_when_chatgpt_forced() {
        let (mut widget, _tmp) = widget_forced_chatgpt();

        widget.save_api_key("sk-test".to_string());

        assert_eq!(widget.error.as_deref(), Some(API_KEY_DISABLED_MESSAGE));
        assert!(matches!(
            &*widget.sign_in_state.read().unwrap(),
            SignInState::PickMode
        ));
        assert_eq!(widget.login_status, LoginStatus::NotAuthenticated);
    }

    /// Collects all buffer cell symbols that contain the OSC 8 open sequence
    /// for the given URL.  Returns the concatenated "inner" characters.
    fn collect_osc8_chars(buf: &Buffer, area: Rect, url: &str) -> String {
        let open = format!("\x1B]8;;{url}\x07");
        let close = "\x1B]8;;\x07";
        let mut chars = String::new();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                let sym = buf[(x, y)].symbol();
                if let Some(rest) = sym.strip_prefix(open.as_str())
                    && let Some(ch) = rest.strip_suffix(close)
                {
                    chars.push_str(ch);
                }
            }
        }
        chars
    }

    #[test]
    fn mark_url_hyperlink_wraps_cyan_underlined_cells() {
        let url = "https://example.com";
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);

        // Manually write some cyan+underlined characters to simulate a rendered URL.
        for (i, ch) in "example".chars().enumerate() {
            let cell = &mut buf[(i as u16, 0)];
            cell.set_symbol(&ch.to_string());
            cell.fg = Color::Cyan;
            cell.modifier = Modifier::UNDERLINED;
        }
        // Leave a plain cell that should NOT be marked.
        buf[(7, 0)].set_symbol("X");

        mark_url_hyperlink(&mut buf, area, url);

        // Each cyan+underlined cell should now carry the OSC 8 wrapper.
        let found = collect_osc8_chars(&buf, area, url);
        assert_eq!(found, "example");

        // The plain "X" cell should be untouched.
        assert_eq!(buf[(7, 0)].symbol(), "X");
    }

    #[test]
    fn mark_url_hyperlink_sanitizes_control_chars() {
        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);

        // One cyan+underlined cell to mark.
        let cell = &mut buf[(0, 0)];
        cell.set_symbol("a");
        cell.fg = Color::Cyan;
        cell.modifier = Modifier::UNDERLINED;

        // URL contains ESC and BEL that could break the OSC 8 sequence.
        let malicious_url = "https://evil.com/\x1B]8;;\x07injected";
        mark_url_hyperlink(&mut buf, area, malicious_url);

        let sym = buf[(0, 0)].symbol().to_string();
        // The sanitized URL retains `]` (printable) but strips ESC and BEL.
        let sanitized = "https://evil.com/]8;;injected";
        assert!(
            sym.contains(sanitized),
            "symbol should contain sanitized URL, got: {sym:?}"
        );
        // The injected close-sequence must not survive: \x1B and \x07 are gone.
        assert!(
            !sym.contains("\x1B]8;;\x07injected"),
            "symbol must not contain raw control chars from URL"
        );
    }
}
