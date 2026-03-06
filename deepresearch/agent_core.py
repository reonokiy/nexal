from dataclasses import asdict, dataclass
import json
import logging
import os
from datetime import datetime
from pathlib import Path
from typing import Any
from uuid import uuid4

from openai import OpenAI
from deepresearch.sandbox import Sandbox, SandboxConfig, SandboxExecRequest
from deepresearch.sandbox.base import SandboxSession
from deepresearch.tools.search import get_search_provider


logger = logging.getLogger("deepresearch.agent")


@dataclass
class AgentSettings:
    llm_api_endpoint: str
    llm_api_key: str
    llm_model: str
    search_provider: str
    sandbox_session_id: str = ""
    sandbox_workspace_read_only: bool = False
    sandbox_workspace_dir: str = ""
    sandbox_network_enabled: bool = False


SYSTEM_PROMPT = """You are a minimal deep research agent.
You have access to a web_search tool, a get_current_datetime tool, and a run_command tool, and should decide for yourself when each is necessary.
Use get_current_datetime for questions about today, now, the current date, the current time, or similar time-relative requests.
Use run_command when you need to execute code or shell commands. Use /workspace as the persistent working directory for files you want to keep across commands.
You must use the web_search tool for time-sensitive questions and for factual questions where freshness, verification, or source grounding matters.
Use the web_search tool when it would materially improve factual accuracy, freshness, or source coverage.
Do not call web_search if the question can be answered reliably without it or by using get_current_datetime.
Treat requests about today, now, current status, latest updates, recent events, prices, rankings, schedules, laws, regulations, product details, company information, and similar topics as search-required.
When you use web_search, cite the source titles and URLs you relied on.
If the search results are weak or incomplete, say so clearly."""

TOOL_SPEC = [
    {
        "type": "function",
        "function": {
            "name": "web_search",
            "description": "Search the web and return a short list of relevant results.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query to look up on the web.",
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum number of search results to return.",
                        "default": 5,
                        "minimum": 1,
                        "maximum": 10,
                    },
                },
                "required": ["query"],
                "additionalProperties": False,
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "get_current_datetime",
            "description": "Get the current local date and time from the system clock.",
            "parameters": {
                "type": "object",
                "properties": {},
                "additionalProperties": False,
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "run_command",
            "description": "Run a command in the persistent working environment. Use /workspace for files you want to keep.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Command and arguments to execute inside the container.",
                    },
                    "workdir": {
                        "type": "string",
                        "description": "Working directory. Prefer paths under /workspace.",
                        "default": "/workspace",
                    },
                    "env": {
                        "type": "object",
                        "description": "Environment variables to pass into the container.",
                        "additionalProperties": {"type": "string"},
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "default": 60,
                        "minimum": 1,
                        "maximum": 600,
                    },
                },
                "required": ["command"],
                "additionalProperties": False,
            },
        },
    }
]


def config() -> AgentSettings:
    endpoint = os.getenv("LLM_ENDPOINT", "https://openrouter.ai/api/v1")
    model = os.getenv("LLM_MODEL", "openai/gpt-4o")
    api_key = os.getenv("LLM_API_KEY")
    search_provider = os.getenv("SEARCH_PROVIDER", "tavily")
    sandbox_session_id = os.getenv("SANDBOX_SESSION_ID", "").strip()
    workspace_read_only_env = os.getenv("SANDBOX_WORKSPACE_READ_ONLY", "").strip().lower()
    sandbox_network_env = os.getenv("SANDBOX_NETWORK_ENABLED", "").strip().lower()
    if not api_key:
        raise RuntimeError("LLM_API_KEY environment variable is required")

    return AgentSettings(
        llm_api_endpoint=endpoint,
        llm_api_key=api_key,
        llm_model=model,
        search_provider=search_provider,
        sandbox_session_id=sandbox_session_id,
        sandbox_workspace_read_only=workspace_read_only_env in {"1", "true", "yes", "on"},
        sandbox_network_enabled=sandbox_network_env in {"1", "true", "yes", "on"},
    )


def ensure_sandbox_session(settings: AgentSettings) -> AgentSettings:
    existing = settings.sandbox_workspace_dir
    if existing:
        return settings

    root = Path(os.getenv("SANDBOX_SESSIONS_DIR", ".sandbox_sessions")).resolve()
    root.mkdir(parents=True, exist_ok=True)
    session_name = settings.sandbox_session_id or uuid4().hex
    workspace_dir = root / session_name
    workspace_dir.mkdir(parents=True, exist_ok=True)

    settings.sandbox_session_id = session_name
    settings.sandbox_workspace_dir = str(workspace_dir)
    logger.info("sandbox_session_ready session_id=%s workspace_dir=%s", session_name, workspace_dir)
    return settings


def init_client(settings: AgentSettings) -> OpenAI:
    return OpenAI(
        base_url=settings.llm_api_endpoint,
        api_key=settings.llm_api_key,
    )


def web_search(provider: str, query: str, max_results: int = 5) -> dict[str, Any]:
    return get_search_provider(provider).search(query, max_results=max_results)


def get_current_datetime() -> dict[str, str]:
    now = datetime.now().astimezone()
    return {
        "iso_datetime": now.isoformat(),
        "date": now.date().isoformat(),
        "time": now.strftime("%H:%M:%S"),
        "weekday": now.strftime("%A"),
        "timezone": str(now.tzinfo),
    }


def _default_network(settings: AgentSettings) -> str:
    return "host" if settings.sandbox_network_enabled else "none"


def get_or_start_sandbox_session(
    settings: AgentSettings,
    session: SandboxSession | None,
) -> SandboxSession:
    if session is not None:
        return session

    sandbox = Sandbox(
        config=SandboxConfig(
            session_id=settings.sandbox_session_id,
            workspace_dir=settings.sandbox_workspace_dir or None,
            workspace_read_only=bool(settings.sandbox_workspace_read_only),
            network=_default_network(settings),
            shared_dirs=[],
        )
    )
    return sandbox.start()


def run_tool(
    tool_name: str,
    arguments: str,
    settings: AgentSettings,
    session: SandboxSession | None = None,
) -> tuple[str, SandboxSession | None]:
    parsed_args = json.loads(arguments or "{}")
    logger.info("tool_call_start name=%s args=%s", tool_name, arguments or "{}")

    if tool_name == "web_search":
        query = str(parsed_args["query"])
        max_results = int(parsed_args.get("max_results", 5))
        tool_output = json.dumps(
            web_search(settings.search_provider, query, max_results=max_results),
            ensure_ascii=False,
        )
        logger.info("tool_call_end name=%s output=%s", tool_name, tool_output)
        return tool_output, session
    if tool_name == "get_current_datetime":
        tool_output = json.dumps(get_current_datetime(), ensure_ascii=False)
        logger.info("tool_call_end name=%s output=%s", tool_name, tool_output)
        return tool_output, session
    if tool_name == "run_command":
        session = get_or_start_sandbox_session(settings, session)
        request = SandboxExecRequest(
            command=[str(part) for part in parsed_args["command"]],
            workdir=str(parsed_args.get("workdir", "/workspace")),
            env={str(k): str(v) for k, v in parsed_args.get("env", {}).items()},
            timeout_seconds=int(parsed_args.get("timeout_seconds", 60)),
        )
        tool_output = json.dumps(asdict(session.exec(request)), ensure_ascii=False)
        logger.info("tool_call_end name=%s output=%s", tool_name, tool_output)
        return tool_output, session

    raise ValueError(f"Unknown tool: {tool_name}")


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
    sandbox_session: SandboxSession | None = None

    messages: list[dict[str, Any]] = [
        {"role": "system", "content": SYSTEM_PROMPT},
        {"role": "user", "content": query},
    ]

    for _ in range(max_turns):
        response = client.chat.completions.create(
            model=settings.llm_model,
            messages=messages,
            tools=TOOL_SPEC,
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
            tool_output, sandbox_session = run_tool(
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
