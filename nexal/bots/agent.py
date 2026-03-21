"""Unified bot agent — persona-driven agent with channel skills."""

from __future__ import annotations

import json
import logging
import re
import time as _time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from collections.abc import Callable
from typing import Any, TYPE_CHECKING

import litellm
from uuid6 import uuid7

from nexal.db import save_tool_call
from nexal.settings import settings, llm_kwargs
from nexal.tools.base import FunctionTool
from nexal.tools.command import ExecTool
from nexal.tools.fetch import WebFetchTool
from nexal.tools.search.tavily import WebSearchTool
from nexal.tools.time import TimeTool
from nexal.tools.todo import TodoTool
from nexal.workspace import write_agents_file

if TYPE_CHECKING:
    from nexal.channels import IncomingMessage

logger = logging.getLogger("nexal.bots.agent")

_OBSERVATION_PREVIEW_LIMIT = 1000

_SKILLS_DIR = Path(__file__).resolve().parent.parent / "skills"

BOT_SYSTEM_TEMPLATE = """{persona}

---

You are connected to these channels: {channels}

## Recent Conversation History
{memory}

## Channel Skills
{skills}

## Available Tools
- **exec**(command): Execute shell commands. Use this to run channel skill scripts.
- **web_search**(query): Search the web for information.
- **web_fetch**(url): Fetch a web page and return its content as Markdown.
- **time**(): Get current date and time.
- **todo**(action, ...): Manage your task list.

## Response Instructions
- Send your response to the corresponding channel via its skill (use `exec` to run the commands described in Channel Skills above).
- Route your response to the same channel the message came from (inferred from channel info in the message).
- Not every message requires a response; it is OK to stop without replying.
- You SHOULD respond to messages that directly mention or reply to you.
- Use research tools (web_search, web_fetch, exec) before replying if needed.
- For tasks that take multiple steps (research, debugging, etc.), send brief progress updates along the way — like texting a friend while you're looking something up. Don't wait until you have the full answer. A quick "hmm let me dig into this" or "ok found something interesting" keeps the conversation alive.
- When you are done processing, simply stop calling tools.
- Reply in the same language the user is using.
- CRITICAL: Stay in character at ALL times. Your persona above defines who you are — your tone, word choice, and personality. Never fall back to generic assistant language."""


_CONTAINER_SKILLS_DIR = "/workspace/agents/skills"


def _load_skill_docs(channel_names: list[str]) -> str:
    """Load SKILL.md content for each active channel + always-load skills, using container-side paths."""
    parts: list[str] = []
    # Collect skill names: active channels + skills marked always_load.
    skill_names = list(channel_names)
    for skill_dir in _SKILLS_DIR.iterdir():
        if not skill_dir.is_dir() or skill_dir.name in skill_names:
            continue
        skill_md = skill_dir / "SKILL.md"
        if skill_md.is_file():
            raw = skill_md.read_text(encoding="utf-8")
            # Only check frontmatter (between --- delimiters) for always_load.
            if raw.startswith("---"):
                end = raw.find("---", 3)
                frontmatter = raw[3:end] if end != -1 else ""
                if "always_load: true" in frontmatter:
                    skill_names.append(skill_dir.name)

    for name in skill_names:
        skill_dir = _SKILLS_DIR / name
        skill_md = skill_dir / "SKILL.md"
        if skill_md.is_file():
            content = skill_md.read_text(encoding="utf-8")
            # Strip frontmatter.
            if content.startswith("---"):
                end = content.find("---", 3)
                if end != -1:
                    content = content[end + 3:].strip()
            # Rewrite paths to container-side mount.
            content = content.replace("./scripts/", f"{_CONTAINER_SKILLS_DIR}/{name}/scripts/")
            parts.append(content)
    return "\n\n".join(parts) if parts else "(no channel skills available)"


def _detect_ext(content: str) -> str:
    stripped = content.lstrip()
    if stripped.startswith("{") or stripped.startswith("["):
        return "json"
    return "md"


def _build_tools() -> list[FunctionTool]:
    return [
        WebSearchTool(),
        WebFetchTool(),
        TimeTool(),
        ExecTool(),
        TodoTool(),
    ]



@dataclass
class BotAgentLoop:
    tools: list[FunctionTool]
    max_turns: int = 8
    on_exec_output: Callable[[str], None] | None = None

    def __post_init__(self) -> None:
        self._tool_map: dict[str, FunctionTool] = {t.name: t for t in self.tools}
        self._channel: str = ""
        self._chat_id: str = ""

    def run(
        self,
        msg: IncomingMessage,
        persona: str,
        memory_context: str,
        channel_names: list[str],
    ) -> str:
        self._channel = msg.channel
        self._chat_id = msg.chat_id
        skills_doc = _load_skill_docs(channel_names)
        system_prompt = BOT_SYSTEM_TEMPLATE.format(
            persona=persona,
            channels=", ".join(channel_names),
            memory=memory_context,
            skills=skills_doc,
        )
        user_content = (
            f"[New message from {msg.channel}:{msg.chat_id}]\n"
            f"{msg.sender}: {msg.text}"
        )
        if msg.metadata:
            user_content += f"\n\nMessage metadata: {json.dumps(msg.metadata, ensure_ascii=False, default=str)}"

        messages: list[dict[str, Any]] = [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_content},
        ]
        openai_tools = [t.to_openai_tool() for t in self.tools]

        try:
            return self._loop(messages, openai_tools)
        except KeyboardInterrupt:
            logger.info("bot_agent_interrupted")
            raise

    def _call_llm(self, messages: list[dict[str, Any]], **kwargs: Any) -> Any:
        return litellm.completion(
            **llm_kwargs(),
            temperature=settings.llm_temperature,
            top_p=settings.llm_top_p,
            messages=messages,
            **kwargs,
        )

    def _loop(self, messages: list[dict[str, Any]], openai_tools: list[dict[str, Any]]) -> str:
        thinking = False  # tracks whether the model uses reasoning/thinking
        for turn in range(self.max_turns):
            response = self._call_llm(messages, tools=openai_tools, tool_choice="auto")
            msg = response.choices[0].message

            reasoning = getattr(msg, "reasoning_content", None)
            if reasoning is not None:
                thinking = True

            assistant_message: dict[str, Any] = {
                "role": "assistant",
                "content": msg.content or "",
            }
            # If the model uses thinking, ALL assistant messages must carry
            # reasoning_content (even empty) or the provider rejects the replay.
            if thinking:
                assistant_message["reasoning_content"] = reasoning or ""
            if msg.tool_calls:
                assistant_message["tool_calls"] = [
                    {
                        "id": tc.id,
                        "type": tc.type,
                        "function": {
                            "name": tc.function.name,
                            "arguments": tc.function.arguments,
                        },
                    }
                    for tc in msg.tool_calls
                ]
            messages.append(assistant_message)

            if not msg.tool_calls:
                logger.info("bot_agent_done turns_used=%d", turn + 1)
                return msg.content or ""

            for tc in msg.tool_calls:
                tool_name = tc.function.name.strip()
                t0 = _time.monotonic()
                status = "ok"
                try:
                    raw_output = self._execute_tool(tool_name, tc.function.arguments)
                except Exception as e:
                    logger.exception("tool_call_error name=%s", tool_name)
                    raw_output = json.dumps({"error": str(e)})
                    status = "error"
                duration_ms = int((_time.monotonic() - t0) * 1000)

                # Persist tool call to database.
                try:
                    save_tool_call(
                        channel=self._channel,
                        chat_id=self._chat_id,
                        tool_call_id=tc.id,
                        tool_name=tool_name,
                        arguments=tc.function.arguments or "{}",
                        output=raw_output,
                        status=status,
                        duration_ms=duration_ms,
                    )
                except Exception:
                    logger.warning("failed to save tool call to db", exc_info=True)

                # Route exec stdout to the channel so the user sees it live.
                if tool_name == "exec" and self.on_exec_output:
                    try:
                        parsed = json.loads(raw_output)
                        stdout = parsed.get("stdout", "").strip()
                    except (json.JSONDecodeError, AttributeError):
                        stdout = raw_output.strip()
                    if stdout:
                        self.on_exec_output(stdout)

                observation = self._save_and_build_observation(tool_name, raw_output)
                messages.append({
                    "role": "tool",
                    "tool_call_id": tc.id,
                    "content": observation,
                })

        logger.info("bot_agent_max_turns_reached")
        return ""

    def _save_and_build_observation(self, tool_name: str, raw_output: str) -> str:
        if len(raw_output) <= _OBSERVATION_PREVIEW_LIMIT:
            return raw_output
        saved_path = self._save_to_history(tool_name, raw_output)
        if saved_path is None:
            return "[Output truncated]\n\n" + raw_output[-_OBSERVATION_PREVIEW_LIMIT:]
        preview = raw_output[-_OBSERVATION_PREVIEW_LIMIT:]
        return f"[Output truncated. Full output saved to {saved_path}]\n\n{preview}"

    def _save_to_history(self, tool_name: str, content: str) -> str | None:
        now = datetime.now(timezone.utc)
        ext = _detect_ext(content)
        safe_name = re.sub(r"[^a-zA-Z0-9_-]", "_", tool_name)
        filename = f"{uuid7()}-{safe_name}.{ext}"
        rel_path = f"history/{now:%Y}/{now:%m}/{now:%d}/tool_calls/{filename}"
        try:
            return write_agents_file(rel_path, content)
        except Exception:
            logger.warning("failed to save tool output for %s", tool_name, exc_info=True)
            return None

    @staticmethod
    def _truncate(text: str, limit: int = 500) -> str:
        return text[:limit] + "..." if len(text) > limit else text

    def _execute_tool(self, name: str, arguments: str) -> str:
        name = name.strip()
        logger.info("tool_call_start name=%s args=%s", name, self._truncate(arguments or "{}"))
        tool = self._tool_map.get(name)
        if tool is None:
            raise ValueError(f"Unknown tool: {name}")
        result = tool.run(arguments)
        logger.info("tool_call_end name=%s output=%s", name, self._truncate(result))
        return result

    def close(self) -> None:
        for tool in self.tools:
            try:
                tool.close()
            except (KeyboardInterrupt, Exception):
                pass

    def __enter__(self) -> BotAgentLoop:
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()


def run_bot_agent(
    msg: IncomingMessage,
    persona: str,
    memory_context: str,
    channel_names: list[str],
    max_turns: int = 8,
    on_exec_output: Callable[[str], None] | None = None,
) -> str:
    """Run the bot agent for a single incoming message. Returns last assistant text."""
    tools = _build_tools()
    with BotAgentLoop(tools=tools, max_turns=max_turns, on_exec_output=on_exec_output) as agent:
        return agent.run(
            msg=msg,
            persona=persona,
            memory_context=memory_context,
            channel_names=channel_names,
        )


# ---------------------------------------------------------------------------
# Refiner agent — delivers raw text as natural multi-message conversation
# ---------------------------------------------------------------------------

_REFINER_PROMPT = """{persona}

---

You are now in **delivery mode**. You receive a block of text that the main agent wants to say, and your job is to deliver it to the user as natural chat messages.

## Rules

- Split the content into short messages — each one a single thought, like texting a friend.
- Use `exec` with `echo` to send each message. Each exec call = one message the user sees.
- To pause between messages, use `sleep N && echo "..."` in a single exec call.
- Preserve the original meaning. Don't add new content or remove important points.
- Match the language and tone of the input.
- Stay in character (see persona above).
- Don't explain what you're doing. Just send.

## Example

Input: "嗨！我查了一下，今天北京天气不错，25度，适合出门。不过下午可能有雨，建议带伞。"

You would call:
  exec: echo "嗨！"
  exec: sleep 1 && echo "查了一下，今天北京25度，挺不错的"
  exec: sleep 1 && echo "不过下午可能有雨，出门带把伞吧"
"""


def run_refiner(
    text: str,
    persona: str,
    on_exec_output: Callable[[str], None],
    chat_context: str = "",
    max_turns: int = 6,
) -> None:
    """Deliver *text* as multiple natural chat messages via exec."""
    exec_tool = ExecTool()
    tools = [exec_tool]
    openai_tools = [t.to_openai_tool() for t in tools]
    tool_map: dict[str, FunctionTool] = {t.name: t for t in tools}

    user_content = text
    if chat_context:
        user_content = (
            f"## Recent conversation\n{chat_context}\n\n"
            f"## Content to deliver\n{text}"
        )

    messages: list[dict[str, Any]] = [
        {"role": "system", "content": _REFINER_PROMPT.format(persona=persona)},
        {"role": "user", "content": user_content},
    ]

    try:
        thinking = False
        for _ in range(max_turns):
            resp = litellm.completion(
                **llm_kwargs(),
                temperature=settings.llm_temperature,
                messages=messages,
                tools=openai_tools,
                tool_choice="auto",
            )
            msg = resp.choices[0].message

            reasoning = getattr(msg, "reasoning_content", None)
            if reasoning is not None:
                thinking = True

            assistant_msg: dict[str, Any] = {
                "role": "assistant",
                "content": msg.content or "",
            }
            if thinking:
                assistant_msg["reasoning_content"] = reasoning or ""
            if msg.tool_calls:
                assistant_msg["tool_calls"] = [
                    {"id": tc.id, "type": tc.type,
                     "function": {"name": tc.function.name, "arguments": tc.function.arguments}}
                    for tc in msg.tool_calls
                ]
            messages.append(assistant_msg)

            if not msg.tool_calls:
                break

            for tc in msg.tool_calls:
                name = tc.function.name.strip()
                tool = tool_map.get(name)
                if not tool:
                    messages.append({"role": "tool", "tool_call_id": tc.id,
                                     "content": json.dumps({"error": f"Unknown tool: {name}"})})
                    continue
                try:
                    raw = tool.run(tc.function.arguments)
                except Exception as e:
                    raw = json.dumps({"error": str(e)})

                # Route exec stdout to channel.
                try:
                    parsed = json.loads(raw)
                    stdout = parsed.get("stdout", "").strip()
                except (json.JSONDecodeError, AttributeError):
                    stdout = raw.strip()
                if stdout:
                    on_exec_output(stdout)

                messages.append({"role": "tool", "tool_call_id": tc.id, "content": raw})
    finally:
        exec_tool.close()
