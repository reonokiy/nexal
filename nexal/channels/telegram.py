"""Telegram channel — message filtering, typing indicator, access control, media support."""

from __future__ import annotations

import asyncio
import contextlib
import logging
from collections.abc import AsyncGenerator
from typing import TYPE_CHECKING, Any

from nexal.channels import chunk_message
from nexal.channels.channel import Channel, IncomingMessage

if TYPE_CHECKING:
    from nexal.channels.channel import OnMessage

logger = logging.getLogger("nexal.channels.telegram")

_MAX_TELEGRAM_LEN = 4096
_GROUP_TYPES = {"group", "supergroup"}


def _message_type(message: Any) -> str:
    """Determine the type of a telegram message."""
    if getattr(message, "text", None):
        return "text"
    if getattr(message, "photo", None):
        return "photo"
    if getattr(message, "audio", None):
        return "audio"
    if getattr(message, "sticker", None):
        return "sticker"
    if getattr(message, "video", None):
        return "video"
    if getattr(message, "voice", None):
        return "voice"
    if getattr(message, "document", None):
        return "document"
    if getattr(message, "video_note", None):
        return "video_note"
    return "unknown"


def _content(message: Any) -> str:
    return (getattr(message, "text", None) or getattr(message, "caption", None) or "").strip()


def _exclude_none(d: dict[str, Any]) -> dict[str, Any]:
    return {k: v for k, v in d.items() if v is not None}


class TelegramChannel(Channel):
    """Telegram channel with message filtering, typing, access control, and media support."""

    def __init__(
        self,
        token: str,
        *,
        bot_name: str = "nexal",
        allow_from: list[str] | None = None,
        allow_chats: list[str] | None = None,
    ) -> None:
        self._token = token
        self._bot_name = bot_name.lower()
        self._allow_from: set[str] | None = set(allow_from) if allow_from else None
        self._allow_chats: set[str] | None = set(allow_chats) if allow_chats else None
        self._bot: object | None = None
        self._dp: object | None = None

    @property
    def name(self) -> str:
        return "telegram"

    # ── Message filtering ────────────────────────────────────────────

    def _is_mentioned(self, message: Any) -> bool:
        """Check if the bot is mentioned in a message (bub-style filtering)."""
        msg_type = _message_type(message)
        if msg_type == "unknown":
            return False

        # Private chat: always process.
        if message.chat.type == "private":
            return True

        # Group chat: only when explicitly addressed.
        if message.chat.type in _GROUP_TYPES:
            bot = message.bot
            bot_id = bot.id
            bot_username = (getattr(bot, "username", "") or "").lower()

            if self._mentions_bot(message, bot_id, bot_username):
                return True
            if self._is_reply_to_bot(message, bot_id):
                return True

            # Non-text media without caption only counts if it's a reply.
            if msg_type != "text" and not getattr(message, "caption", None):
                return False

            return False

        return False

    def _mentions_bot(self, message: Any, bot_id: int, bot_username: str) -> bool:
        content = _content(message).lower()

        # Check keyword or @username mention in text.
        if self._bot_name in content or (bot_username and f"@{bot_username}" in content):
            return True

        # Check entities for structured mentions.
        entities = [
            *(getattr(message, "entities", None) or ()),
            *(getattr(message, "caption_entities", None) or ()),
        ]
        for entity in entities:
            if entity.type == "mention" and bot_username:
                mention_text = content[entity.offset : entity.offset + entity.length]
                if mention_text == f"@{bot_username}":
                    return True
            if entity.type == "text_mention" and getattr(entity, "user", None) and entity.user.id == bot_id:
                return True

        return False

    @staticmethod
    def _is_reply_to_bot(message: Any, bot_id: int) -> bool:
        reply = getattr(message, "reply_to_message", None)
        if reply is None:
            return False
        from_user = getattr(reply, "from_user", None)
        if from_user is None:
            return False
        return from_user.id == bot_id

    # ── Access control ───────────────────────────────────────────────

    def _check_access(self, message: Any) -> bool:
        """Return True if the sender is allowed to interact."""
        chat_id = str(message.chat.id)

        # If allow_chats is set and this chat is in it, allow.
        if self._allow_chats and chat_id in self._allow_chats:
            return True

        # If allow_chats is set but chat not in it, deny.
        if self._allow_chats and chat_id not in self._allow_chats:
            return False

        # If allow_from is set, check sender.
        if self._allow_from:
            user = getattr(message, "from_user", None)
            if user is None:
                return False
            sender_tokens = {str(user.id)}
            if getattr(user, "username", None):
                sender_tokens.add(user.username)
            return not sender_tokens.isdisjoint(self._allow_from)

        # No restrictions configured.
        return True

    # ── Typing indicator ─────────────────────────────────────────────

    @contextlib.asynccontextmanager
    async def _typing(self, chat_id: str | int) -> AsyncGenerator[None, None]:
        bot = self._bot
        typing_task = asyncio.create_task(self._typing_loop(bot, chat_id))
        try:
            yield
        finally:
            typing_task.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await typing_task

    @staticmethod
    async def _typing_loop(bot: Any, chat_id: str | int) -> None:
        from aiogram.enums import ChatAction

        try:
            while True:
                await bot.send_chat_action(chat_id=chat_id, action=ChatAction.TYPING)
                await asyncio.sleep(4)
        except asyncio.CancelledError:
            return
        except Exception:
            logger.debug("typing_loop_error chat=%s", chat_id, exc_info=True)
            return

    # ── Media parsing ────────────────────────────────────────────────

    @classmethod
    def _parse_message(cls, message: Any) -> tuple[str, dict[str, Any] | None]:
        msg_type = _message_type(message)
        if msg_type == "text":
            return message.text or "", None
        parser = cls._MEDIA_PARSERS.get(msg_type)
        if parser is not None:
            return parser(message)
        return "[Unknown message type]", None

    @staticmethod
    def _parse_photo(message: Any) -> tuple[str, dict[str, Any] | None]:
        caption = getattr(message, "caption", None) or ""
        formatted = f"[Photo] Caption: {caption}" if caption else "[Photo]"
        photos = getattr(message, "photo", None) or []
        if not photos:
            return formatted, None
        largest = photos[-1]
        meta = _exclude_none({
            "file_id": largest.file_id,
            "file_size": getattr(largest, "file_size", None),
            "width": largest.width,
            "height": largest.height,
        })
        return formatted, meta

    @staticmethod
    def _parse_audio(message: Any) -> tuple[str, dict[str, Any] | None]:
        audio = getattr(message, "audio", None)
        if audio is None:
            return "[Audio]", None
        title = getattr(audio, "title", None) or "Unknown"
        performer = getattr(audio, "performer", None) or ""
        duration = getattr(audio, "duration", 0) or 0
        meta = _exclude_none({
            "file_id": audio.file_id,
            "file_size": getattr(audio, "file_size", None),
            "duration": duration,
            "title": getattr(audio, "title", None),
            "performer": getattr(audio, "performer", None),
        })
        if performer:
            return f"[Audio: {performer} - {title} ({duration}s)]", meta
        return f"[Audio: {title} ({duration}s)]", meta

    @staticmethod
    def _parse_sticker(message: Any) -> tuple[str, dict[str, Any] | None]:
        sticker = getattr(message, "sticker", None)
        if sticker is None:
            return "[Sticker]", None
        emoji = getattr(sticker, "emoji", None) or ""
        set_name = getattr(sticker, "set_name", None) or ""
        meta = _exclude_none({
            "file_id": sticker.file_id,
            "width": sticker.width,
            "height": sticker.height,
            "emoji": emoji or None,
            "set_name": set_name or None,
            "is_animated": getattr(sticker, "is_animated", None),
            "is_video": getattr(sticker, "is_video", None),
        })
        if emoji:
            return f"[Sticker: {emoji} from {set_name}]", meta
        return f"[Sticker from {set_name}]", meta

    @staticmethod
    def _parse_video(message: Any) -> tuple[str, dict[str, Any] | None]:
        video = getattr(message, "video", None)
        duration = getattr(video, "duration", 0) if video else 0
        caption = getattr(message, "caption", None) or ""
        formatted = f"[Video: {duration}s]"
        if caption:
            formatted = f"{formatted} Caption: {caption}"
        if video is None:
            return formatted, None
        meta = _exclude_none({
            "file_id": video.file_id,
            "file_size": getattr(video, "file_size", None),
            "width": getattr(video, "width", None),
            "height": getattr(video, "height", None),
            "duration": duration,
        })
        return formatted, meta

    @staticmethod
    def _parse_voice(message: Any) -> tuple[str, dict[str, Any] | None]:
        voice = getattr(message, "voice", None)
        duration = getattr(voice, "duration", 0) if voice else 0
        if voice is None:
            return f"[Voice: {duration}s]", None
        meta = _exclude_none({"file_id": voice.file_id, "duration": duration})
        return f"[Voice: {duration}s]", meta

    @staticmethod
    def _parse_document(message: Any) -> tuple[str, dict[str, Any] | None]:
        doc = getattr(message, "document", None)
        if doc is None:
            return "[Document]", None
        file_name = getattr(doc, "file_name", None) or "unknown"
        mime_type = getattr(doc, "mime_type", None) or "unknown"
        caption = getattr(message, "caption", None) or ""
        formatted = f"[Document: {file_name} ({mime_type})]"
        if caption:
            formatted = f"{formatted} Caption: {caption}"
        meta = _exclude_none({
            "file_id": doc.file_id,
            "file_name": getattr(doc, "file_name", None),
            "file_size": getattr(doc, "file_size", None),
            "mime_type": getattr(doc, "mime_type", None),
        })
        return formatted, meta

    @staticmethod
    def _parse_video_note(message: Any) -> tuple[str, dict[str, Any] | None]:
        vn = getattr(message, "video_note", None)
        duration = getattr(vn, "duration", 0) if vn else 0
        if vn is None:
            return f"[Video note: {duration}s]", None
        meta = _exclude_none({"file_id": vn.file_id, "duration": duration})
        return f"[Video note: {duration}s]", meta

    _MEDIA_PARSERS: dict[str, Any] = {
        "photo": _parse_photo,
        "audio": _parse_audio,
        "sticker": _parse_sticker,
        "video": _parse_video,
        "voice": _parse_voice,
        "document": _parse_document,
        "video_note": _parse_video_note,
    }

    # ── Reply metadata ───────────────────────────────────────────────

    @staticmethod
    def _extract_reply_metadata(message: Any) -> dict[str, Any] | None:
        reply = getattr(message, "reply_to_message", None)
        if reply is None:
            return None
        from_user = getattr(reply, "from_user", None)
        if from_user is None:
            return None
        return _exclude_none({
            "message_id": reply.message_id,
            "from_user_id": from_user.id,
            "from_username": getattr(from_user, "username", None),
            "from_is_bot": getattr(from_user, "is_bot", None),
            "text": (getattr(reply, "text", "") or "")[:100],
        })

    # ── Start / Send / Stop ──────────────────────────────────────────

    async def start(self, on_message: OnMessage) -> None:
        from aiogram import Bot, Dispatcher, types

        bot = Bot(self._token)
        dp = Dispatcher()
        self._bot = bot
        self._dp = dp

        @dp.message()
        async def handler(message: types.Message) -> None:
            try:
                # Access control.
                if not self._check_access(message):
                    logger.info("telegram_access_denied chat=%s user=%s", message.chat.id, getattr(message.from_user, "id", "?"))
                    return

                # Parse content.
                text, media = self._parse_message(message)
                if not text.strip() and media is None:
                    return

                is_mentioned = self._is_mentioned(message)

                sender = message.from_user
                sender_name = sender.full_name if sender else "Unknown"
                chat_title = message.chat.title if message.chat.title else "DM"

                logger.info(
                    "telegram_message chat=%s sender=%s mentioned=%s text=%s",
                    chat_title, sender_name, is_mentioned, text[:100],
                )

                # Build metadata.
                metadata: dict[str, Any] = {
                    "chat_title": chat_title,
                    "message_id": message.message_id,
                    "type": _message_type(message),
                    "username": sender.username if sender else "",
                    "sender_id": str(sender.id) if sender else "",
                    "sender_is_bot": sender.is_bot if sender else None,
                }
                if media:
                    metadata["media"] = media
                    caption = getattr(message, "caption", None)
                    if caption:
                        metadata["caption"] = caption
                reply_meta = self._extract_reply_metadata(message)
                if reply_meta:
                    metadata["reply_to_message"] = reply_meta

                msg = IncomingMessage(
                    channel="telegram",
                    chat_id=str(message.chat.id),
                    sender=sender_name,
                    text=text,
                    timestamp=message.date,
                    is_mentioned=is_mentioned,
                    metadata=metadata,
                    typing_fn=lambda: self._typing(message.chat.id),
                )

                await on_message(msg)
            except Exception:
                logger.exception("telegram_handler_error chat=%s", message.chat.id)

        logger.info(
            "telegram_channel_starting allow_from=%s allow_chats=%s",
            len(self._allow_from) if self._allow_from else "all",
            len(self._allow_chats) if self._allow_chats else "all",
        )
        await dp.start_polling(bot, drop_pending_updates=True)

    async def send(self, chat_id: str, text: str) -> None:
        from aiogram import Bot

        bot: Bot = self._bot  # type: ignore[assignment]
        if bot is None:
            raise RuntimeError("Telegram channel not started")
        chunks = chunk_message(text, _MAX_TELEGRAM_LEN)
        for chunk in chunks:
            await bot.send_message(int(chat_id), chunk)

    async def stop(self) -> None:
        if self._dp is not None:
            from aiogram import Dispatcher
            dp: Dispatcher = self._dp  # type: ignore[assignment]
            await dp.stop_polling()
