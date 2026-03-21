"""Unified CLI entry point.

- Default: daemon mode (Telegram / Discord channels from env).
- ``-i``: interactive terminal mode via prompt_toolkit.
"""

import argparse
import asyncio
import logging

from nexal.settings import settings, load_settings


def main() -> None:
    parser = argparse.ArgumentParser(description="Run the Nexal agent")
    parser.add_argument(
        "-i", "--interactive", action="store_true",
        help="Enter interactive CLI mode",
    )
    parser.add_argument("--session", help="Optional sandbox session id to reuse")
    parser.add_argument(
        "--workspace-readonly", action="store_true",
        help="Mount the default sandbox /workspace as read-only",
    )
    parser.add_argument(
        "--sandbox-network", action="store_true",
        help="Enable network access for sandbox exec calls",
    )
    parser.add_argument(
        "--max-turns", type=int, default=10,
        help="Maximum agent turns per message (default: 10)",
    )
    args = parser.parse_args()

    # In interactive mode the TUI installs its own log handler;
    # only set up console logging for daemon mode.
    if not args.interactive:
        logging.basicConfig(
            level=logging.INFO,
            format="%(asctime)s %(levelname)s %(name)s %(message)s",
        )
    else:
        logging.root.setLevel(logging.INFO)

    # Force LiteLLM to use our logging format instead of its own handlers.
    _tame_litellm_logging()

    load_settings()
    if args.session:
        settings.sandbox_session_id = args.session
    if args.workspace_readonly:
        settings.sandbox_workspace_read_only = True
    if args.sandbox_network:
        settings.sandbox_network_enabled = True

    from nexal.bots.bot import Bot

    bot = Bot(max_turns=args.max_turns)

    _add_daemon_channels(bot)
    if args.interactive:
        from nexal.channels.cli import CLIChannel
        bot.add_channel(CLIChannel())

    if not bot.channels:
        raise RuntimeError(
            "No channels configured. Set TELEGRAM_BOT_TOKEN / DISCORD_BOT_TOKEN, "
            "or use -i for interactive mode."
        )

    try:
        asyncio.run(bot.start())
    except KeyboardInterrupt:
        logging.getLogger("nexal").info("shutting down")


class _StripNewlineFilter(logging.Filter):
    """Strip leading/trailing whitespace from log messages."""
    def filter(self, record: logging.LogRecord) -> bool:
        if isinstance(record.msg, str):
            record.msg = record.msg.strip()
        return True


def _tame_litellm_logging() -> None:
    """Strip LiteLLM's custom handlers so its logs use our root format."""
    import litellm as _  # noqa: F401 — ensure the module registers its loggers first
    for name in ("LiteLLM", "litellm"):
        lg = logging.getLogger(name)
        lg.handlers.clear()
        lg.propagate = True
        lg.addFilter(_StripNewlineFilter())


def _add_daemon_channels(bot) -> None:  # noqa: ANN001
    """Register external channels based on environment config."""
    log = logging.getLogger("nexal.bots")

    if settings.telegram_bot_token:
        from nexal.channels.telegram import TelegramChannel
        bot.add_channel(TelegramChannel(
            settings.telegram_bot_token,
            bot_name=settings.bot_name,
            allow_from=settings.telegram_allow_from,
            allow_chats=settings.telegram_allow_chats,
        ))
        log.info("telegram channel enabled")

    if settings.discord_bot_token:
        from nexal.channels.discord import DiscordChannel
        bot.add_channel(DiscordChannel(
            settings.discord_bot_token,
            bot_name=settings.bot_name,
            allow_from=settings.discord_allow_from,
            allow_channels=settings.discord_allow_channels,
        ))
        log.info("discord channel enabled")
