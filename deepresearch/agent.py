import json
import logging
from dataclasses import dataclass, field
from typing import Any

from openai import OpenAI
from deepresearch.prompts import SYSTEM_PROMPT
from deepresearch.settings import AgentSettings, ensure_sandbox_session
from deepresearch.tools.base import FunctionTool
from deepresearch.tools.registry import get_default_tools


logger = logging.getLogger("deepresearch.agent")


def init_client(settings: AgentSettings) -> OpenAI:
    return OpenAI(
        base_url=settings.llm_api_endpoint,
        api_key=settings.llm_api_key,
    )


@dataclass
class AgentLoop:
    client: OpenAI
    settings: AgentSettings
    tools: list[FunctionTool] = field(default_factory=get_default_tools)
    max_turns: int = 8

    def __post_init__(self) -> None:
        self.settings = ensure_sandbox_session(self.settings)
        self._tool_map: dict[str, FunctionTool] = {t.name: t for t in self.tools}

    def run(self, query: str) -> str:
        logger.info(
            "agent_start session_id=%s workspace_dir=%s",
            self.settings.sandbox_session_id,
            self.settings.sandbox_workspace_dir,
        )
        messages: list[dict[str, Any]] = [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": query},
        ]

        try:
            for _ in range(self.max_turns):
                response = self.client.chat.completions.create(
                    model=self.settings.llm_model,
                    messages=messages,  # type: ignore[arg-type]
                    tools=[t.to_openai_tool() for t in self.tools],  # type: ignore[list-item]
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

                if not message.tool_calls:
                    return message.content or ""

                for tc in message.tool_calls:
                    try:
                        output = self._execute_tool(tc.function.name, tc.function.arguments)
                    except Exception as e:
                        logger.exception("tool_call_error name=%s", tc.function.name)
                        output = json.dumps({"error": str(e)})
                    messages.append({
                        "role": "tool",
                        "tool_call_id": tc.id,
                        "content": output,
                    })

            raise RuntimeError("Agent exceeded max_turns without producing a final answer")
        finally:
            self.close()

    def _execute_tool(self, name: str, arguments: str) -> str:
        logger.info("tool_call_start name=%s args=%s", name, arguments or "{}")
        tool = self._tool_map.get(name)
        if tool is None:
            raise ValueError(f"Unknown tool: {name}")
        result = tool.execute(arguments, self.settings)
        logger.info("tool_call_end name=%s output=%s", name, result)
        return result

    def close(self) -> None:
        for tool in self.tools:
            tool.close()


def run_agent(
    client: OpenAI,
    settings: AgentSettings,
    query: str,
    max_turns: int = 8,
) -> str:
    if not logger.handlers:
        logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(name)s %(message)s")
    return AgentLoop(client=client, settings=settings, max_turns=max_turns).run(query)
