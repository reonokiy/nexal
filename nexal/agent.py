import json
import logging
import re
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Any

from uuid6 import uuid7
from openai import OpenAI

from nexal.prompts import SYSTEM_PROMPT
from nexal.settings import settings, ensure_sandbox_session
from nexal.tools.base import FunctionTool
from nexal.tools.registry import get_default_tools
from nexal.workspace import write_agents_file


logger = logging.getLogger("nexal.agent")
logging.getLogger("httpx").setLevel(logging.WARNING)

_OBSERVATION_PREVIEW_LIMIT = 1000


def init_client() -> OpenAI:
    return OpenAI(
        base_url=settings.llm_api_endpoint,
        api_key=settings.llm_api_key,
    )


def _detect_ext(content: str) -> str:
    stripped = content.lstrip()
    if stripped.startswith("{") or stripped.startswith("["):
        return "json"
    return "md"


@dataclass
class AgentLoop:
    client: OpenAI
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

        for _ in range(self.max_turns):
            # --- Action: model decides to call tools or give final answer ---
            response = self.client.chat.completions.create(
                model=settings.llm_model,
                messages=messages,  # type: ignore[arg-type]
                tools=openai_tools,  # type: ignore[list-item]
                tool_choice="auto",
            )
            message = response.choices[0].message

            assistant_message: dict[str, Any] = {
                "role": "assistant",
                "content": message.content or "",
            }
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

            # No tool calls → final answer.
            if not message.tool_calls:
                return message.content or ""

            # --- Observation: execute tools, collect results ---
            for tc in message.tool_calls:
                try:
                    raw_output = self._execute_tool(tc.function.name, tc.function.arguments)
                except Exception as e:
                    logger.exception("tool_call_error name=%s", tc.function.name)
                    raw_output = json.dumps({"error": str(e)})

                observation = self._save_and_build_observation(tc.function.name, raw_output)
                messages.append({
                    "role": "tool",
                    "tool_call_id": tc.id,
                    "content": observation,
                })

            # --- Thought prompt: guide the model to reflect before next action ---
            messages.append({
                "role": "user",
                "content": "Based on the observations above, briefly state what you learned and what you should do next. Then take your next action or provide a final answer.",
            })

        # Ask the model to summarize what it has so far.
        messages.append({"role": "user", "content": "You have reached the maximum number of steps. Please summarize your findings so far and provide the best answer you can."})
        response = self.client.chat.completions.create(
            model=settings.llm_model,
            messages=messages,  # type: ignore[arg-type]
        )
        return response.choices[0].message.content or ""

    def _save_and_build_observation(self, tool_name: str, raw_output: str) -> str:
        """Save raw tool output to history file and return a truncated observation."""
        # For short outputs, just return as-is.
        if len(raw_output) <= _OBSERVATION_PREVIEW_LIMIT:
            return raw_output

        # Save full output to file.
        saved_path = self._save_to_history(tool_name, raw_output)
        if saved_path is None:
            # Fallback: truncate inline if saving failed.
            return raw_output[:_OBSERVATION_PREVIEW_LIMIT] + "\n\n[Output truncated]"

        preview = raw_output[:_OBSERVATION_PREVIEW_LIMIT]
        return f"{preview}\n\n[Output truncated. Full output saved to {saved_path} — use exec to view if needed]"

    def _save_to_history(self, tool_name: str, content: str) -> str | None:
        """Save content to /workspace/agents/history/YYYY/MM/DD/<uuid7>-<name>.<ext>.

        Returns the container-side path or None on failure.
        """
        now = datetime.now(timezone.utc)
        ext = _detect_ext(content)
        safe_name = re.sub(r"[^a-zA-Z0-9_-]", "_", tool_name)
        filename = f"{uuid7()}-{safe_name}.{ext}"
        rel_path = f"history/{now:%Y}/{now:%m}/{now:%d}/{filename}"

        try:
            return write_agents_file(rel_path, content)
        except Exception:
            logger.warning("failed to save tool output for %s", tool_name, exc_info=True)
            return None

    @staticmethod
    def _truncate(text: str, limit: int = 500) -> str:
        return text[:limit] + "..." if len(text) > limit else text

    def _execute_tool(self, name: str, arguments: str) -> str:
        logger.info("tool_call_start name=%s args=%s", name, self._truncate(arguments or "{}"))
        tool = self._tool_map.get(name)
        if tool is None:
            raise ValueError(f"Unknown tool: {name}")
        result = tool.run(arguments)
        logger.info("tool_call_end name=%s output=%s", name, self._truncate(result))
        return result

    def close(self) -> None:
        for tool in self.tools:
            tool.close()

    def __enter__(self) -> "AgentLoop":
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()


def run_agent(query: str, max_turns: int = 8) -> str:
    if not logging.root.handlers:
        logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(name)s %(message)s")
    with AgentLoop(client=init_client(), max_turns=max_turns) as loop:
        return loop.run(query)
