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

    load_settings()
    if args.session:
        settings.sandbox_session_id = args.session
    if args.workspace_readonly:
        settings.sandbox_workspace_read_only = True
    if args.sandbox_network:
        settings.sandbox_network_enabled = True

    from nexal.bots.bot import Bot

    bot = Bot(max_turns=args.max_turns)

    if args.interactive:
        from nexal.channels.cli import CLIChannel
        bot.add_channel(CLIChannel())
    else:
        _add_daemon_channels(bot)

    try:
        asyncio.run(bot.start())
    except KeyboardInterrupt:
        logging.getLogger("nexal").info("shutting down")


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

    if not bot.channels:
        raise RuntimeError(
            "No channels configured. Set TELEGRAM_BOT_TOKEN / DISCORD_BOT_TOKEN, "
            "or use -i for interactive mode."
        )
