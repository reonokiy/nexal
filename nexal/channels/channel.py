from __future__ import annotations

import contextlib
from abc import ABC, abstractmethod
from collections.abc import AsyncGenerator
from dataclasses import dataclass, field
from datetime import datetime
from typing import Any, Callable, Coroutine


OnMessage = Callable[["IncomingMessage"], Coroutine[Any, Any, None]]


@dataclass
class ImageAttachment:
    """An image downloaded from a channel message."""
    data: bytes
    mime_type: str
    filename: str = ""


@dataclass
class IncomingMessage:
    channel: str
    chat_id: str
    sender: str
    text: str
    timestamp: datetime
    is_mentioned: bool = True
    metadata: dict[str, Any] = field(default_factory=dict)
    images: list[ImageAttachment] = field(default_factory=list)
    typing_fn: Callable[[], contextlib.AbstractAsyncContextManager[None]] | None = field(
        default=None, repr=False, compare=False,
    )


class Channel(ABC):
    """Abstract communication channel that can listen for and send messages."""

    @property
    @abstractmethod
    def name(self) -> str: ...

    @property
    def direct_response(self) -> bool:
        """If True, the agent's final text output is sent via ``send()`` automatically.

        Channels that use external skill scripts (Telegram, Discord) return False.
        Channels where the agent's text IS the reply (CLI) return True.
        """
        return False

    @abstractmethod
    async def start(self, on_message: OnMessage) -> None:
        """Start listening for messages. Calls on_message for each incoming message.

        This method should run indefinitely (e.g. polling loop).
        """

    @abstractmethod
    async def send(self, chat_id: str, text: str) -> None:
        """Send a message to the given chat."""

    async def stop(self) -> None:
        """Stop the channel gracefully."""
