"""Session runner — debounces and batches messages per session (ported from bub)."""

from __future__ import annotations

import asyncio
import logging
from typing import Any, Callable, Coroutine

from nexal.channels.channel import IncomingMessage

logger = logging.getLogger("nexal.channels.runner")

OnProcessMessage = Callable[["IncomingMessage"], Coroutine[Any, Any, None]]


class SessionRunner:
    """Per-session message aggregation with debouncing.

    - When bot is mentioned: wait debounce_seconds before processing.
    - Follow-up messages (after mention, within active window): wait message_delay_seconds.
    - Messages outside active window and not mentioned: ignored.
    """

    def __init__(
        self,
        session_id: str,
        handler: OnProcessMessage,
        *,
        debounce_seconds: int = 1,
        message_delay_seconds: int = 10,
        active_time_window_seconds: int = 60,
    ) -> None:
        self.session_id = session_id
        self._handler = handler
        self.debounce_seconds = debounce_seconds
        self.message_delay_seconds = message_delay_seconds
        self.active_time_window_seconds = active_time_window_seconds

        self._pending: list[IncomingMessage] = []
        self._event = asyncio.Event()
        self._timer: asyncio.TimerHandle | None = None
        self._last_mentioned_at: float | None = None
        self._running_task: asyncio.Task[None] | None = None
        self._loop = asyncio.get_running_loop()

    def _reset_timer(self, timeout: int) -> None:
        self._event.clear()
        if self._timer:
            self._timer.cancel()
        self._timer = self._loop.call_later(timeout, self._event.set)

    async def _run(self) -> None:
        """Wait for the debounce timer, then process all pending messages."""
        await self._event.wait()
        messages = list(self._pending)
        self._pending.clear()
        self._running_task = None

        if not messages:
            return

        # Process the last message (which has the most context).
        # Merge earlier messages into its text.
        if len(messages) == 1:
            msg = messages[0]
        else:
            combined_text = "\n".join(m.text for m in messages)
            merged_metadata: dict = {}
            for m in messages:
                if m.metadata:
                    merged_metadata.update(m.metadata)
            last = messages[-1]
            msg = IncomingMessage(
                channel=last.channel,
                chat_id=last.chat_id,
                sender=last.sender,
                text=combined_text,
                timestamp=last.timestamp,
                is_mentioned=True,
                metadata=merged_metadata,
                typing_fn=last.typing_fn,
            )

        try:
            await self._handler(msg)
        except Exception:
            logger.exception("session_runner_error session_id=%s", self.session_id)

    async def process_message(self, msg: IncomingMessage) -> None:
        """Process an incoming message with debouncing logic."""
        now = self._loop.time()

        if not msg.is_mentioned:
            # Not mentioned — only process if within active time window.
            if (
                self._last_mentioned_at is None
                or now - self._last_mentioned_at > self.active_time_window_seconds
            ):
                self._last_mentioned_at = None
                logger.info("session_runner_ignored session_id=%s", self.session_id)
                return

        self._pending.append(msg)

        if msg.is_mentioned:
            # Mentioned: short debounce.
            self._last_mentioned_at = now
            logger.info("session_runner_mentioned session_id=%s", self.session_id)
            self._reset_timer(self.debounce_seconds)
            if self._running_task is None:
                self._running_task = asyncio.create_task(self._run())
            return await self._running_task
        elif self._last_mentioned_at is not None and self._running_task is None:
            # Follow-up within active window: longer delay.
            logger.info("session_runner_followup session_id=%s", self.session_id)
            self._reset_timer(self.message_delay_seconds)
            self._running_task = asyncio.create_task(self._run())
            return await self._running_task
