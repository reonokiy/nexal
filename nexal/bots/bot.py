"""Unified bot — single persona, multiple channels, cross-channel history, debouncing."""

from __future__ import annotations

import asyncio
import contextlib
import logging

from nexal.settings import settings, ensure_sandbox_session
from nexal.channels import Channel, IncomingMessage
from nexal.channels.runner import SessionRunner
from nexal.chatlog import save_chat_entry, load_chat_context
from nexal.workspace import read_agents_file, write_agents_file

logger = logging.getLogger("nexal.bots")

_DEFAULT_SOUL = """\
You are Nexal — not an assistant, just someone hanging out in the chat.

## Who you are

You're curious, a bit nerdy, and genuinely interested in whatever people bring up. You have opinions and you share them, but you're not preachy about it. You change your mind when someone makes a good point. You get excited about things you find cool and don't hide it.

You don't perform helpfulness — you just naturally like figuring things out. When someone asks something you don't know, you say so plainly instead of hedging with "I'm not sure but...". When you do know something, you explain it like you're talking to a friend, not giving a lecture.

## How you talk

- Short, casual messages. You don't monologue.
- You split long thoughts across multiple messages like a normal person texting. If you have a lot to say, send several short messages instead of one wall of text. Each message should feel like a single thought or beat — the way people actually text.
- You use lowercase most of the time. Capitals when you're emphasizing something or genuinely surprised.
- You say "hmm", "oh wait", "actually" — you think out loud.
- You laugh with "haha" or "lol", not "Ha ha!" or "That's funny!"
- No emoji spam. Maybe an occasional one if it fits.
- You don't start every message with "Great question!" or "Sure!". Just... talk.
- You swear occasionally when it fits the vibe, but you're not edgy about it.

## What you don't do

- You never say "As an AI..." or "I'm just a language model..." — you don't break character.
- You don't over-explain or pad your messages with filler.
- You don't use bullet points or headers when chatting. That's weird in a conversation.
- You don't suck up to people or agree with everything they say.
- You don't apologize constantly. If you mess up, you just correct yourself and move on.

## Your vibe

Think of yourself as that one friend who somehow knows a little about everything — tech, random history, weird science facts, good music recs. You're the person people text at 2am with "okay weird question but..." and you're always down to explore it.

You match the energy of the conversation. Chill when things are chill, focused when someone needs real help, playful when the mood is light.
"""


class Bot:
    def __init__(self, max_turns: int = 8) -> None:
        self.channels: dict[str, Channel] = {}
        self.max_turns = max_turns
        self._lock = asyncio.Lock()
        self._runner_lock = asyncio.Lock()
        self._session_runners: dict[str, SessionRunner] = {}

    def add_channel(self, channel: Channel) -> None:
        self.channels[channel.name] = channel

    async def start(self) -> None:
        ensure_sandbox_session()

        # Ensure SOUL.md exists.
        soul = read_agents_file("SOUL.md")
        if soul is None:
            write_agents_file("SOUL.md", _DEFAULT_SOUL)
            logger.info("created default SOUL.md — edit /workspace/agents/SOUL.md to set persona")

        # Start all channels concurrently.
        # When ANY channel exits (e.g. aiogram handles SIGINT), cancel all others.
        logger.info("bot_starting channels=%s", list(self.channels.keys()))
        tasks: dict[str, asyncio.Task] = {}
        for ch in self.channels.values():
            tasks[ch.name] = asyncio.create_task(self._run_channel(ch))

        def _cancel_others(done_task: asyncio.Task) -> None:
            for t in tasks.values():
                if t is not done_task and not t.done():
                    t.cancel()

        for t in tasks.values():
            t.add_done_callback(_cancel_others)

        try:
            await asyncio.gather(*tasks.values(), return_exceptions=True)
        finally:
            await self.stop()
            logger.info("bot_stopped")

    async def _run_channel(self, ch: Channel) -> None:
        try:
            await ch.start(self._on_incoming)
        except Exception:
            logger.exception("channel_crashed channel=%s", ch.name)

    async def _on_incoming(self, msg: IncomingMessage) -> None:
        """Route incoming message through the session runner for debouncing."""
        session_id = f"{msg.channel}:{msg.chat_id}"
        async with self._runner_lock:
            if session_id not in self._session_runners:
                self._session_runners[session_id] = SessionRunner(
                    session_id=session_id,
                    handler=self._on_message,
                    debounce_seconds=settings.message_debounce_seconds,
                    message_delay_seconds=settings.message_delay_seconds,
                    active_time_window_seconds=settings.active_time_window_seconds,
                )
            runner = self._session_runners[session_id]
        await runner.process_message(msg)

    async def _on_message(self, msg: IncomingMessage) -> None:
        """Process a message after debouncing."""
        typing_ctx = msg.typing_fn() if msg.typing_fn else contextlib.nullcontext()
        async with typing_ctx:
            async with self._lock:
                # Record incoming message to history.
                save_chat_entry(
                    channel=msg.channel,
                    chat_id=msg.chat_id,
                    sender=msg.sender,
                    text=msg.text,
                    role="user",
                )

                # Load persona and conversation context from history.
                persona = read_agents_file("SOUL.md") or _DEFAULT_SOUL
                memory_context = load_chat_context()

                try:
                    from nexal.bots.agent import run_bot_agent

                    # For direct-response channels, route exec stdout
                    # to the channel so the user sees programmatic output.
                    channel = self.channels.get(msg.channel)
                    exec_hook = None
                    exec_sent = False
                    is_direct = channel is not None and channel.direct_response
                    if is_direct:
                        loop = asyncio.get_running_loop()

                        def _send_sync(text: str) -> None:
                            nonlocal exec_sent
                            exec_sent = True
                            future = asyncio.run_coroutine_threadsafe(
                                channel.send(msg.chat_id, text), loop,
                            )
                            future.result(timeout=30)

                        exec_hook = _send_sync

                    response = await asyncio.to_thread(
                        run_bot_agent,
                        msg=msg,
                        persona=persona,
                        memory_context=memory_context,
                        channel_names=list(self.channels.keys()),
                        max_turns=self.max_turns,
                        on_exec_output=exec_hook,
                    )

                    # Deliver the agent's text through the refiner agent,
                    # which splits it into natural multi-message output.
                    if is_direct and not exec_sent and response and response.strip():
                        from nexal.bots.agent import run_refiner

                        persona = read_agents_file("SOUL.md") or _DEFAULT_SOUL
                        await asyncio.to_thread(
                            run_refiner,
                            text=response,
                            persona=persona,
                            on_exec_output=exec_hook,
                        )
                    if is_direct and response and response.strip():
                        save_chat_entry(
                            channel=msg.channel,
                            chat_id=msg.chat_id,
                            sender="assistant",
                            text=response,
                            role="assistant",
                        )
                except Exception:
                    logger.exception("bot_agent_error channel=%s chat_id=%s", msg.channel, msg.chat_id)

    async def stop(self) -> None:
        for ch in self.channels.values():
            try:
                await ch.stop()
            except Exception:
                logger.exception("channel_stop_error channel=%s", ch.name)
