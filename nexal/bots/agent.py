"""Unified bot agent — persona-driven agent with channel skills."""

from __future__ import annotations

import json
import logging
import re
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, TYPE_CHECKING

import litellm
from uuid6 import uuid7

from nexal.settings import settings
from nexal.tools.base import FunctionTool
from nexal.tools.command import ExecTool
from nexal.tools.fetch import WebFetchTool
from nexal.tools.final_answer import FinalAnswerTool
from nexal.tools.search.tavily import WebSearchTool
from nexal.tools.time import TimeTool
from nexal.tools.todo import TodoTool
from nexal.workspace import write_agents_file

if TYPE_CHECKING:
    from nexal.channels import IncomingMessage

logger = logging.getLogger("nexal.bots.agent")

_OBSERVATION_PREVIEW_LIMIT = 1000
_FINAL_ANSWER_TOOL = "final_answer"

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
- **final_answer**(answer): Call when you're done processing this message. Pass empty string if no action needed.

## Response Instructions
- You MUST send a message to the corresponding channel via its skill script before calling final_answer when you want to respond.
- Route your response to the same channel the message came from (inferred from channel info in the message).
- There is a skill for each channel — use `exec` to run the skill scripts.
- Not every message requires a response; it is OK to finish without replying.
- You SHOULD respond to messages that directly mention or reply to you.
- Use research tools (web_search, web_fetch, exec) before replying if needed.
- For tasks that take multiple steps (research, debugging, etc.), send brief progress updates along the way — like texting a friend while you're looking something up. Don't wait until you have the full answer. A quick "hmm let me dig into this" or "ok found something interesting" keeps the conversation alive.
- Call **final_answer** when you're finished processing.
- Reply in the same language the user is using.
- CRITICAL: Stay in character at ALL times. Your persona above defines who you are — your tone, word choice, and personality. Never fall back to generic assistant language."""


_CONTAINER_SKILLS_DIR = "/workspace/agents/skills"


def _load_skill_docs(channel_names: list[str]) -> str:
    """Load SKILL.md content for each active channel, using container-side paths."""
    parts: list[str] = []
    for name in channel_names:
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
        FinalAnswerTool(),
    ]


def _llm_kwargs() -> dict[str, Any]:
    """Common kwargs for litellm.completion calls."""
    kwargs: dict[str, Any] = {"model": settings.llm_model, "timeout": 300.0}
    if settings.llm_api_key:
        kwargs["api_key"] = settings.llm_api_key
    if settings.llm_api_base:
        kwargs["api_base"] = settings.llm_api_base
    return kwargs


@dataclass
class BotAgentLoop:
    tools: list[FunctionTool]
    max_turns: int = 8

    def __post_init__(self) -> None:
        self._tool_map: dict[str, FunctionTool] = {t.name: t for t in self.tools}

    def run(
        self,
        msg: IncomingMessage,
        persona: str,
        memory_context: str,
        channel_names: list[str],
    ) -> None:
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
            self._loop(messages, openai_tools)
        except KeyboardInterrupt:
            logger.info("bot_agent_interrupted")
            raise

    def _call_llm(self, messages: list[dict[str, Any]], **kwargs: Any) -> Any:
        return litellm.completion(
            **_llm_kwargs(),
            temperature=settings.llm_temperature,
            top_p=settings.llm_top_p,
            messages=messages,
            **kwargs,
        )

    def _loop(self, messages: list[dict[str, Any]], openai_tools: list[dict[str, Any]]) -> None:
        for turn in range(self.max_turns):
            response = self._call_llm(messages, tools=openai_tools, tool_choice="auto")
            msg = response.choices[0].message

            assistant_message: dict[str, Any] = {
                "role": "assistant",
                "content": msg.content or "",
            }
            # Preserve reasoning_content for models that use thinking/reasoning.
            reasoning = getattr(msg, "reasoning_content", None)
            if reasoning:
                assistant_message["reasoning_content"] = reasoning
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
                logger.info("bot_agent_thinking turn=%d", turn + 1)
                continue

            for tc in msg.tool_calls:
                tool_name = tc.function.name.strip()
                try:
                    raw_output = self._execute_tool(tool_name, tc.function.arguments)
                except Exception as e:
                    logger.exception("tool_call_error name=%s", tool_name)
                    raw_output = json.dumps({"error": str(e)})

                if tool_name == _FINAL_ANSWER_TOOL:
                    logger.info("bot_agent_done turns_used=%d", turn + 1)
                    return

                observation = self._save_and_build_observation(tool_name, raw_output)
                messages.append({
                    "role": "tool",
                    "tool_call_id": tc.id,
                    "content": observation,
                })

        logger.info("bot_agent_max_turns_reached")

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
) -> None:
    """Run the bot agent for a single incoming message."""
    tools = _build_tools()
    with BotAgentLoop(tools=tools, max_turns=max_turns) as agent:
        agent.run(
            msg=msg,
            persona=persona,
            memory_context=memory_context,
            channel_names=channel_names,
        )
