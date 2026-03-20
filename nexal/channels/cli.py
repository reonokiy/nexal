"""CLI channel — full-screen TUI with a log pane and a chat pane.

Layout::

    ┌──────────── nexal ────────────┐
    │  logs (dim, scrollable)       │
    ├───────────────────────────────┤
    │  chat messages                │
    ├───────────────────────────────┤
    │  > input                      │
    └───────────────────────────────┘

Output (logs, agent responses) never blocks the input prompt.
"""

from __future__ import annotations

import asyncio
import io
import logging
import re
import sys
from datetime import datetime, timezone

from prompt_toolkit.application import Application
from prompt_toolkit.document import Document
from prompt_toolkit.key_binding import KeyBindings
from prompt_toolkit.layout.containers import HSplit, Window, WindowAlign
from prompt_toolkit.layout.controls import FormattedTextControl
from prompt_toolkit.layout.dimension import Dimension
from prompt_toolkit.layout.layout import Layout
from prompt_toolkit.styles import Style
from prompt_toolkit.widgets import TextArea

from nexal.channels.channel import Channel, IncomingMessage, OnMessage

logger = logging.getLogger("nexal.channels.cli")

_ANSI_RE = re.compile(r"\x1b\[[0-9;]*m")


def _strip_ansi(text: str) -> str:
    return _ANSI_RE.sub("", text)


_MAX_BUFFER_LINES = 2000


def _buffer_append(text_area: TextArea, text: str) -> None:
    """Append text to a read-only TextArea buffer, capping at _MAX_BUFFER_LINES."""
    buf = text_area.buffer
    was_readonly = buf.read_only
    buf.read_only = lambda: False
    try:
        new = buf.text + text + "\n"
        # Trim to last N lines to prevent unbounded growth.
        lines = new.split("\n")
        if len(lines) > _MAX_BUFFER_LINES:
            new = "\n".join(lines[-_MAX_BUFFER_LINES:])
        buf.document = Document(text=new, cursor_position=len(new))
    finally:
        buf.read_only = was_readonly


# ---------------------------------------------------------------------------
# stdout/stderr → log pane
# ---------------------------------------------------------------------------

class _TUIStream(io.TextIOBase):
    """Redirect writes (stdout/stderr) into a TextArea via the event loop."""

    def __init__(self, text_area: TextArea, loop: asyncio.AbstractEventLoop) -> None:
        self._text_area = text_area
        self._loop = loop

    def write(self, s: str) -> int:
        if s and not s.isspace():
            clean = _strip_ansi(s.rstrip("\n"))
            if clean and not clean.isspace():
                self._loop.call_soon_threadsafe(_buffer_append, self._text_area, clean)
        return len(s)

    def flush(self) -> None:
        pass

    @property
    def encoding(self) -> str:
        return "utf-8"


# ---------------------------------------------------------------------------
# logging → log pane
# ---------------------------------------------------------------------------

class _TUILogHandler(logging.Handler):
    """Append log lines to a prompt_toolkit TextArea (thread-safe)."""

    def __init__(self, text_area: TextArea, loop: asyncio.AbstractEventLoop) -> None:
        super().__init__()
        self._text_area = text_area
        self._loop = loop

    def emit(self, record: logging.LogRecord) -> None:
        try:
            msg = _strip_ansi(self.format(record))
            self._loop.call_soon_threadsafe(_buffer_append, self._text_area, msg)
        except Exception:
            self.handleError(record)


# ---------------------------------------------------------------------------
# CLI Channel
# ---------------------------------------------------------------------------

class CLIChannel(Channel):
    """Full-screen TUI channel with separate log and chat panes."""

    def __init__(self, user_name: str = "user") -> None:
        self._user_name = user_name
        self._on_message: OnMessage | None = None
        self._app: Application | None = None
        self._quit_pending = False

        # --- widgets ---
        self._log_area = TextArea(
            height=Dimension(weight=2),
            style="class:log-area",
            read_only=True,
            scrollbar=True,
        )
        self._chat_area = TextArea(
            height=Dimension(weight=8),
            style="class:chat-area",
            read_only=True,
            scrollbar=True,
        )
        self._input_field = TextArea(
            height=1,
            prompt="› ",
            style="class:input-field",
            multiline=False,
            wrap_lines=False,
        )
        self._input_field.accept_handler = self._on_accept

    # -- Channel interface --------------------------------------------------

    @property
    def name(self) -> str:
        return "cli"

    @property
    def direct_response(self) -> bool:
        return True

    async def start(self, on_message: OnMessage) -> None:
        self._on_message = on_message
        loop = asyncio.get_running_loop()

        # Key bindings
        kb = KeyBindings()

        @kb.add("c-c", eager=True)
        def _ctrl_c(event):  # noqa: ANN001
            if self._quit_pending:
                event.app.exit()
            else:
                self._quit_pending = True
                self._append_chat("Press Enter or Ctrl-C again to quit.")
                if self._app:
                    self._app.invalidate()

        @kb.add("<any>")
        def _any_key(event):  # noqa: ANN001
            self._quit_pending = False
            event.current_buffer.insert_text(event.data)

        @kb.add("c-q", eager=True)
        def _ctrl_q(event):  # noqa: ANN001
            event.app.exit()

        # Layout
        container = HSplit([
            Window(
                height=1,
                content=FormattedTextControl(
                    [("class:title", " nexal "), ("class:hint", "ctrl-c to quit")],
                ),
                style="class:title-bar",
            ),
            self._log_area,
            Window(height=1, char="─", style="class:separator"),
            self._chat_area,
            Window(height=1, char="─", style="class:separator"),
            self._input_field,
        ])

        style = Style([
            ("title-bar", "bg:#262626 #707070"),
            ("title", "bold #a0a0a0"),
            ("hint", "#505050"),
            ("log-area", "#505050"),
            ("chat-area", ""),
            ("input-field", "#e0e0e0"),
            ("separator", "#303030"),
            ("scrollbar.background", "bg:#1a1a1a"),
            ("scrollbar.button", "bg:#404040"),
        ])

        self._app = Application(
            layout=Layout(container, focused_element=self._input_field),
            key_bindings=kb,
            style=style,
            mouse_support=True,
            full_screen=True,
        )

        # Redirect logging to the log pane
        handler = _TUILogHandler(self._log_area, loop)
        handler.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(name)s %(message)s"))
        logging.root.addHandler(handler)
        # Remove all StreamHandlers (root + third-party like LiteLLM)
        # so nothing writes directly to stdout/stderr.
        for name in (None, "LiteLLM", "litellm"):
            lg = logging.getLogger(name)
            for h in lg.handlers[:]:
                if isinstance(h, logging.StreamHandler) and h is not handler:
                    lg.removeHandler(h)

        # Capture stdout/stderr (e.g. litellm prints) into the log pane
        orig_stdout, orig_stderr = sys.stdout, sys.stderr
        tui_stream = _TUIStream(self._log_area, loop)
        sys.stdout = tui_stream  # type: ignore[assignment]
        sys.stderr = tui_stream  # type: ignore[assignment]

        logger.info("cli_ready")
        try:
            await self._app.run_async()
        finally:
            sys.stdout, sys.stderr = orig_stdout, orig_stderr

    async def send(self, chat_id: str, text: str) -> None:
        self._append_chat(f"nexal: {text}")

    async def stop(self) -> None:
        if self._app and self._app.is_running:
            self._app.exit()

    # -- Internal -----------------------------------------------------------

    def _on_accept(self, buff) -> None:  # noqa: ANN001
        """Called synchronously by prompt_toolkit when the user presses Enter."""
        text = self._input_field.text.strip()

        if self._quit_pending:
            if self._app:
                self._app.exit()
            return

        if not text:
            return
        if text.lower() in ("exit", "quit"):
            if self._app:
                self._app.exit()
            return

        self._append_chat(f"{self._user_name}: {text}")

        msg = IncomingMessage(
            channel="cli",
            chat_id="terminal",
            sender=self._user_name,
            text=text,
            timestamp=datetime.now(timezone.utc),
            is_mentioned=True,
        )
        if self._on_message:
            asyncio.ensure_future(self._on_message(msg))

    def _append_chat(self, text: str) -> None:
        _buffer_append(self._chat_area, text)
        if self._app:
            self._app.invalidate()
