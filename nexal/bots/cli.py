"""CLI entry point for the unified bot."""

import argparse
import asyncio
import logging

from nexal.settings import settings, load_settings


def main() -> None:
    parser = argparse.ArgumentParser(description="Run the unified Nexal bot")
    parser.add_argument(
        "--max-turns", type=int, default=8,
        help="Maximum agent turns per message (default: 8)",
    )
    parser.add_argument(
        "--sandbox-network", action="store_true",
        help="Enable network access for sandbox",
    )
    args = parser.parse_args()

    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(levelname)s %(name)s %(message)s",
    )

    load_settings()
    if args.sandbox_network:
        settings.sandbox_network_enabled = True

    from nexal.bots.bot import Bot

    bot = Bot(max_turns=args.max_turns)

    # Add channels based on available tokens.
    if settings.telegram_bot_token:
        from nexal.channels.telegram import TelegramChannel
        bot.add_channel(TelegramChannel(
            settings.telegram_bot_token,
            bot_name=settings.bot_name,
            allow_from=settings.telegram_allow_from,
            allow_chats=settings.telegram_allow_chats,
        ))
        logging.getLogger("nexal.bots").info("telegram channel enabled")

    if settings.discord_bot_token:
        from nexal.channels.discord import DiscordChannel
        bot.add_channel(DiscordChannel(
            settings.discord_bot_token,
            bot_name=settings.bot_name,
            allow_from=settings.discord_allow_from,
            allow_channels=settings.discord_allow_channels,
        ))
        logging.getLogger("nexal.bots").info("discord channel enabled")

    if not bot.channels:
        # No external channels — enable interactive CLI channel as fallback.
        from nexal.channels.cli import CLIChannel
        bot.add_channel(CLIChannel())
        logging.getLogger("nexal.bots").info("cli channel enabled (no external channels configured)")

    try:
        asyncio.run(bot.start())
    except KeyboardInterrupt:
        logging.getLogger("nexal.bots").info("bot shutting down")


if __name__ == "__main__":
    main()
