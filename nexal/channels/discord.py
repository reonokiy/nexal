"""Discord channel — mention detection, typing indicator, access control, media support."""

from __future__ import annotations

import logging
from collections import OrderedDict
from typing import TYPE_CHECKING, Any

from nexal.channels import chunk_message
from nexal.channels.channel import Channel, IncomingMessage

if TYPE_CHECKING:
    from nexal.channels.channel import OnMessage

logger = logging.getLogger("nexal.channels.discord")

_MAX_DISCORD_LEN = 2000


def _message_type(message: Any) -> str:
    if message.content:
        return "text"
    if message.attachments:
        return "attachment"
    if message.stickers:
        return "sticker"
    return "unknown"


def _exclude_none(d: dict[str, Any]) -> dict[str, Any]:
    return {k: v for k, v in d.items() if v is not None}


class DiscordChannel(Channel):
    """Discord channel with mention detection, typing, access control, and media support."""

    def __init__(
        self,
        token: str,
        *,
        bot_name: str = "nexal",
        allow_from: list[str] | None = None,
        allow_channels: list[str] | None = None,
    ) -> None:
        self._token = token
        self._bot_name = bot_name.lower()
        self._allow_from: set[str] | None = set(allow_from) if allow_from else None
        self._allow_channels: set[str] | None = set(allow_channels) if allow_channels else None
        self._bot: object | None = None
        self._latest_message: OrderedDict[str, object] = OrderedDict()
        self._max_cached_messages = 500

    @property
    def name(self) -> str:
        return "discord"

    # ── Mention detection ────────────────────────────────────────────

    def _is_mentioned(self, message: Any) -> bool:
        """Determine if the bot is being addressed (bub-style filtering)."""
        import discord

        channel_id = str(message.channel.id)

        # Access control: channel filter.
        if self._allow_channels and channel_id not in self._allow_channels:
            return False

        if not (message.content or "").strip():
            return False

        # Access control: sender filter.
        if self._allow_from:
            sender_tokens = {str(message.author.id), message.author.name}
            if getattr(message.author, "global_name", None):
                sender_tokens.add(message.author.global_name)
            if sender_tokens.isdisjoint(self._allow_from):
                return False

        # DMs: always mentioned.
        if isinstance(message.channel, discord.DMChannel):
            return True

        # Keyword mention.
        if self._bot_name in message.content.lower():
            return True

        # Bot-scoped thread.
        if self._is_bot_scoped_thread(message):
            return True

        # @mention.
        bot_user = getattr(self._bot, "user", None)
        if bot_user is not None and bot_user in message.mentions:
            return True

        # Reply to bot.
        if bot_user is not None:
            ref = message.reference
            if ref is not None:
                resolved = ref.resolved
                if isinstance(resolved, discord.Message) and resolved.author and resolved.author.id == bot_user.id:
                    return True

        return False

    def _is_bot_scoped_thread(self, message: Any) -> bool:
        import discord
        channel = message.channel
        thread_name = getattr(channel, "name", None)
        if not isinstance(thread_name, str):
            return False
        is_thread = isinstance(channel, discord.Thread) or getattr(channel, "parent", None) is not None
        return is_thread and thread_name.lower().startswith(self._bot_name)

    # ── Media parsing ────────────────────────────────────────────────

    @staticmethod
    def _parse_message(message: Any) -> tuple[str, dict[str, Any] | None]:
        if message.content:
            return message.content, None

        if message.attachments:
            lines: list[str] = []
            meta_list: list[dict[str, Any]] = []
            for att in message.attachments:
                lines.append(f"[Attachment: {att.filename}]")
                meta_list.append(_exclude_none({
                    "id": str(att.id),
                    "filename": att.filename,
                    "content_type": att.content_type,
                    "size": att.size,
                    "url": att.url,
                }))
            return "\n".join(lines), {"attachments": meta_list}

        if message.stickers:
            lines = [f"[Sticker: {s.name}]" for s in message.stickers]
            meta = [{"id": str(s.id), "name": s.name} for s in message.stickers]
            return "\n".join(lines), {"stickers": meta}

        return "[Unknown message type]", None

    @staticmethod
    def _extract_reply_metadata(message: Any) -> dict[str, Any] | None:
        import discord
        ref = message.reference
        if ref is None:
            return None
        resolved = ref.resolved
        if not isinstance(resolved, discord.Message):
            return None
        return _exclude_none({
            "message_id": str(resolved.id),
            "from_user_id": str(resolved.author.id),
            "from_username": resolved.author.name,
            "from_is_bot": resolved.author.bot,
            "text": (resolved.content or "")[:100],
        })

    # ── Start / Send / Stop ──────────────────────────────────────────

    async def start(self, on_message: OnMessage) -> None:
        import discord

        intents = discord.Intents.default()
        intents.message_content = True
        bot = discord.Client(intents=intents)
        self._bot = bot

        @bot.event
        async def on_ready() -> None:
            logger.info("discord_ready user=%s id=%s", bot.user, bot.user.id if bot.user else "?")

        @bot.event
        async def on_message(message: discord.Message) -> None:
            # Ignore own messages.
            if message.author == bot.user:
                return
            # Ignore other bots.
            if message.author.bot:
                return

            text, media = self._parse_message(message)
            if not text.strip() and media is None:
                return

            is_mentioned = self._is_mentioned(message)

            sender_name = message.author.display_name
            channel_name = getattr(message.channel, "name", "DM")

            logger.info(
                "discord_message channel=%s sender=%s mentioned=%s text=%s",
                channel_name, sender_name, is_mentioned, text[:100],
            )

            # Build metadata.
            metadata: dict[str, Any] = {
                "channel_name": channel_name,
                "message_id": message.id,
                "type": _message_type(message),
                "username": message.author.name,
                "sender_id": str(message.author.id),
                "channel_id": str(message.channel.id),
                "guild_id": str(message.guild.id) if message.guild else None,
            }
            if media:
                metadata["media"] = media
            reply_meta = self._extract_reply_metadata(message)
            if reply_meta:
                metadata["reply_to_message"] = reply_meta

            session_id = f"discord:{message.channel.id}"
            self._latest_message[session_id] = message
            if len(self._latest_message) > self._max_cached_messages:
                self._latest_message.popitem(last=False)

            msg = IncomingMessage(
                channel="discord",
                chat_id=str(message.channel.id),
                sender=sender_name,
                text=text,
                timestamp=message.created_at,
                is_mentioned=is_mentioned,
                metadata=_exclude_none(metadata),
                typing_fn=lambda: message.channel.typing(),
            )

            await on_message(msg)

        logger.info(
            "discord_channel_starting allow_from=%s allow_channels=%s",
            len(self._allow_from) if self._allow_from else "all",
            len(self._allow_channels) if self._allow_channels else "all",
        )
        await bot.start(self._token)

    async def send(self, chat_id: str, text: str) -> None:
        import discord

        bot: discord.Client = self._bot  # type: ignore[assignment]
        if bot is None:
            raise RuntimeError("Discord channel not started")

        channel = bot.get_channel(int(chat_id))
        if channel is None:
            channel = await bot.fetch_channel(int(chat_id))

        # Try to reply to the latest message in this session.
        session_id = f"discord:{chat_id}"
        source = self._latest_message.get(session_id)
        reference = None
        if source is not None and hasattr(source, "to_reference"):
            reference = source.to_reference(fail_if_not_exists=False)

        chunks = chunk_message(text, _MAX_DISCORD_LEN)
        for chunk in chunks:
            kwargs: dict[str, Any] = {"content": chunk}
            if reference is not None:
                kwargs["reference"] = reference
                kwargs["mention_author"] = False
            await channel.send(**kwargs)  # type: ignore[union-attr]

    async def stop(self) -> None:
        if self._bot is not None:
            import discord
            bot: discord.Client = self._bot  # type: ignore[assignment]
            await bot.close()
