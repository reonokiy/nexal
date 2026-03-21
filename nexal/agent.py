import json
import logging
import re
import time as _time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Any

import litellm
from uuid6 import uuid7

from nexal.db import save_tool_call
from nexal.prompts import SYSTEM_PROMPT, CONTEXT_COMPRESSION_PROMPT
from nexal.settings import settings, ensure_sandbox_session, llm_kwargs
from nexal.tools.base import FunctionTool
from nexal.tools.registry import get_default_tools
from nexal.workspace import write_agents_file


logger = logging.getLogger("nexal.agent")
logging.getLogger("httpx").setLevel(logging.WARNING)

_OBSERVATION_PREVIEW_LIMIT = 1000



def _detect_ext(content: str) -> str:
    stripped = content.lstrip()
    if stripped.startswith("{") or stripped.startswith("["):
        return "json"
    return "md"


_FINAL_ANSWER_TOOL = "final_answer"


@dataclass
class AgentLoop:
    tools: list[FunctionTool] = field(default_factory=get_default_tools)
    max_turns: int = 8

    def __post_init__(self) -> None:
        ensure_sandbox_session()
        self._tool_map: dict[str, FunctionTool] = {t.name: t for t in self.tools}

    def run(self, query: str) -> str:
        logger.info(
            "agent_start session_id=%s workspace_dir=%s",
            settings.sandbox_session_id,
            settings.sandbox_workspace_dir,
        )
        messages: list[dict[str, Any]] = [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": query},
        ]

        openai_tools = [t.to_openai_tool() for t in self.tools]

        try:
            return self._loop(messages, openai_tools)
        except KeyboardInterrupt:
            logger.info("agent_interrupted")
            raise

    @staticmethod
    def _estimate_tokens(messages: list[dict[str, Any]]) -> int:
        """Rough token estimate: ~4 chars per token."""
        total_chars = 0
        for msg in messages:
            total_chars += len(msg.get("content", "") or "")
            for tc in msg.get("tool_calls", []):
                total_chars += len(tc.get("function", {}).get("arguments", ""))
        return total_chars // 4

    def _call_llm(self, messages: list[dict[str, Any]], **kwargs: Any) -> Any:
        """Call the LLM, compressing context proactively or on overflow."""
        # Proactive compression at 90% of max context.
        threshold = int(settings.llm_max_context_tokens * 0.9)
        if self._estimate_tokens(messages) > threshold:
            logger.warning("context_approaching_limit estimated=%d threshold=%d, compressing",
                           self._estimate_tokens(messages), threshold)
            self._compress_context(messages)

        try:
            return litellm.completion(
                **llm_kwargs(),
                temperature=settings.llm_temperature,
                top_p=settings.llm_top_p,
                messages=messages,
                **kwargs,
            )
        except Exception as e:
            error_str = str(e).lower()
            if "context_length" in error_str or "too many tokens" in error_str or "maximum context" in error_str:
                logger.warning("context_length_exceeded, compressing conversation")
                self._compress_context(messages)
                return litellm.completion(
                    **llm_kwargs(),
                    temperature=settings.llm_temperature,
                    top_p=settings.llm_top_p,
                    messages=messages,
                    **kwargs,
                )
            raise

    def _compress_context(self, messages: list[dict[str, Any]]) -> None:
        """Compress the middle of the conversation in-place, keeping system + user query."""
        # messages[0] = system, messages[1] = original user query, rest = conversation
        if len(messages) <= 3:
            return

        original_query = messages[1]["content"]
        middle = messages[2:]

        # Save full conversation to history before compressing.
        full_dump = json.dumps(messages, ensure_ascii=False, indent=2)
        saved = self._save_to_history("context_pre_compression", full_dump)
        if saved:
            logger.info("context_saved_before_compression path=%s", saved)

        # Build a text representation of the conversation to summarize.
        conversation_text = []
        for msg in middle:
            role = msg.get("role", "unknown")
            content = msg.get("content", "")
            if role == "assistant" and msg.get("tool_calls"):
                tool_names = [tc["function"]["name"] for tc in msg["tool_calls"]]
                conversation_text.append(f"[Assistant called tools: {', '.join(tool_names)}]")
            if content:
                # Limit each message to avoid blowing up the summary request itself.
                conversation_text.append(f"[{role}] {content[:500]}")

        summary_request = CONTEXT_COMPRESSION_PROMPT.format(
            original_query=original_query,
            conversation="\n".join(conversation_text),
        )

        try:
            resp = litellm.completion(
                **llm_kwargs(),
                messages=[{"role": "user", "content": summary_request}],
            )
            summary = resp.choices[0].message.content or ""
        except Exception:
            logger.warning("context_compression_failed, falling back to hard truncation")
            # Fallback: keep only the last few messages.
            keep_last = min(4, len(middle))
            del messages[2:-keep_last]
            return

        # Replace middle messages with a single summary message.
        del messages[2:]
        messages.append({
            "role": "user",
            "content": f"[Context compressed — summary of prior research]\n"
                       f"The conversation history was too long and has been compressed. "
                       f"The full original context was saved to {saved or 'history (save failed)'}.\n\n"
                       f"{summary}\n\n"
                       f"Please continue working on the original task based on this summary.",
        })
        logger.info("context_compressed old_messages=%d new_summary_len=%d", len(middle), len(summary))

    def _loop(self, messages: list[dict[str, Any]], openai_tools: list[dict[str, Any]]) -> str:
        thinking = False
        for _ in range(self.max_turns):
            response = self._call_llm(
                messages,
                tools=openai_tools,
                tool_choice="auto",
            )
            message = response.choices[0].message
            logger.debug("llm_response content=%s tool_calls=%s",
                         self._truncate(message.content or ""), message.tool_calls)

            reasoning = getattr(message, "reasoning_content", None)
            if reasoning is not None:
                thinking = True

            assistant_message: dict[str, Any] = {
                "role": "assistant",
                "content": message.content or "",
            }
            if thinking:
                assistant_message["reasoning_content"] = reasoning or ""
            if message.tool_calls:
                assistant_message["tool_calls"] = [
                    {
                        "id": tc.id,
                        "type": tc.type,
                        "function": {
                            "name": tc.function.name,
                            "arguments": tc.function.arguments,
                        },
                    }
                    for tc in message.tool_calls
                ]
            messages.append(assistant_message)

            if not message.tool_calls:
                # Model returned text without tool calls — treat as thought and continue.
                logger.info("agent_no_tool_calls turn=%d, treating as thought", _ + 1)
                continue

            # --- Execute tools ---
            for tc in message.tool_calls:
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
                        channel="agent",
                        chat_id=settings.sandbox_session_id or "default",
                        tool_call_id=tc.id,
                        tool_name=tool_name,
                        arguments=tc.function.arguments or "{}",
                        output=raw_output,
                        status=status,
                        duration_ms=duration_ms,
                    )
                except Exception:
                    logger.warning("failed to save tool call to db", exc_info=True)

                if tool_name == _FINAL_ANSWER_TOOL:
                    logger.info("agent_finish turns_used=%d", _ + 1)
                    return raw_output

                observation = self._save_and_build_observation(tool_name, raw_output)
                messages.append({
                    "role": "tool",
                    "tool_call_id": tc.id,
                    "content": observation,
                })

        # Max turns reached — force final answer.
        final_tool = self._tool_map.get(_FINAL_ANSWER_TOOL)
        if not final_tool:
            return "Max turns reached."
        final_openai_tool = final_tool.to_openai_tool()
        messages.append({"role": "user", "content": "You have reached the maximum number of steps. You MUST use final_answer now to provide your best answer."})
        try:
            response = self._call_llm(
                messages,
                tools=[final_openai_tool],
                tool_choice={"type": "function", "function": {"name": _FINAL_ANSWER_TOOL}},
            )
        except Exception as e:
            if "tool_choice" in str(e).lower() or "400" in str(e):
                logger.info("forced tool_choice not supported, falling back to auto")
                response = self._call_llm(
                    messages,
                    tools=[final_openai_tool],
                    tool_choice="auto",
                )
            else:
                raise
        message = response.choices[0].message
        if message.tool_calls:
            for tc in message.tool_calls:
                if tc.function.name.strip() == _FINAL_ANSWER_TOOL:
                    return self._execute_tool(_FINAL_ANSWER_TOOL, tc.function.arguments)
        return message.content or ""

    def _save_and_build_observation(self, tool_name: str, raw_output: str) -> str:
        """Save raw tool output to history file and return a truncated observation."""
        # For short outputs, just return as-is.
        if len(raw_output) <= _OBSERVATION_PREVIEW_LIMIT:
            return raw_output

        # Save full output to file.
        saved_path = self._save_to_history(tool_name, raw_output)
        if saved_path is None:
            # Fallback: truncate inline if saving failed.
            return "[Output truncated]\n\n" + raw_output[-_OBSERVATION_PREVIEW_LIMIT:]

        preview = raw_output[-_OBSERVATION_PREVIEW_LIMIT:]
        return f"[Output truncated. Full output saved to {saved_path} — use exec to view if needed]\n\n{preview}"

    def _save_to_history(self, tool_name: str, content: str) -> str | None:
        """Save content to /workspace/agents/history/YYYY/MM/DD/<uuid7>-<name>.<ext>.

        Returns the container-side path or None on failure.
        """
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

    def __enter__(self) -> "AgentLoop":
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()


def run_agent(query: str, max_turns: int = 8) -> str:
    if not logging.root.handlers:
        logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(name)s %(message)s")
    with AgentLoop(max_turns=max_turns) as loop:
        return loop.run(query)
