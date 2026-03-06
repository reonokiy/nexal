import logging
from typing import Any

from openai import OpenAI
from deepresearch.prompts import SYSTEM_PROMPT
from deepresearch.settings import AgentSettings, ensure_sandbox_session
from deepresearch.tool_manager import ToolManager


logger = logging.getLogger("deepresearch.agent")


def init_client(settings: AgentSettings) -> OpenAI:
    return OpenAI(
        base_url=settings.llm_api_endpoint,
        api_key=settings.llm_api_key,
    )

def run_agent(
    client: OpenAI,
    settings: AgentSettings,
    query: str,
    max_turns: int = 8,
) -> str:
    if not logger.handlers:
        logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(name)s %(message)s")

    settings = ensure_sandbox_session(settings)
    logger.info(
        "agent_start session_id=%s workspace_dir=%s",
        settings.sandbox_session_id,
        settings.sandbox_workspace_dir,
    )
    tool_manager = ToolManager()
    sandbox_session = None

    messages: list[dict[str, Any]] = [
        {"role": "system", "content": SYSTEM_PROMPT},
        {"role": "user", "content": query},
    ]

    try:
        for _ in range(max_turns):
            response = client.chat.completions.create(
                model=settings.llm_model,
                messages=messages,
                tools=tool_manager.openai_tools,
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
                        "id": tool_call.id,
                        "type": tool_call.type,
                        "function": {
                            "name": tool_call.function.name,
                            "arguments": tool_call.function.arguments,
                        },
                    }
                    for tool_call in message.tool_calls
                ]

            messages.append(assistant_message)

            if not message.tool_calls:
                return message.content or ""

            for tool_call in message.tool_calls:
                tool_output, sandbox_session = tool_manager.execute(
                    tool_call.function.name,
                    tool_call.function.arguments,
                    settings,
                    sandbox_session,
                )
                messages.append(
                    {
                        "role": "tool",
                        "tool_call_id": tool_call.id,
                        "content": tool_output,
                    }
                )

        raise RuntimeError("Agent exceeded max_turns without producing a final answer")
    finally:
        if sandbox_session is not None:
            try:
                stop_result = sandbox_session.stop()
                logger.info(
                    "sandbox_session_stopped session_id=%s exit_code=%s",
                    stop_result.session_id,
                    stop_result.exit_code,
                )
            except Exception:
                logger.exception(
                    "sandbox_session_stop_failed session_id=%s",
                    getattr(sandbox_session, "session_id", "<unknown>"),
                )
