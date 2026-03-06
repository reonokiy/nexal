import json
import os
from typing import Any

from openai import OpenAI
from search import get_search_provider


SYSTEM_PROMPT = """You are a minimal deep research agent.
Use the web_search tool whenever the user asks for factual, recent, or external information.
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
    }
]


def config() -> dict[str, str]:
    endpoint = os.getenv("LLM_ENDPOINT", "https://openrouter.ai/api/v1")
    model = os.getenv("LLM_MODEL", "openai/gpt-4o")
    api_key = os.getenv("LLM_API_KEY")
    search_provider = os.getenv("SEARCH_PROVIDER", "tavily")
    if not api_key:
        raise RuntimeError("LLM_API_KEY environment variable is required")

    return {
        "llm_api_endpoint": endpoint,
        "llm_api_key": api_key,
        "llm_model": model,
        "search_provider": search_provider,
    }


def init_client(settings: dict[str, str]) -> OpenAI:
    return OpenAI(
        base_url=settings["llm_api_endpoint"],
        api_key=settings["llm_api_key"],
    )


def web_search(provider: str, query: str, max_results: int = 5) -> dict[str, Any]:
    return get_search_provider(provider).search(query, max_results=max_results)


def run_tool(tool_name: str, arguments: str, settings: dict[str, str]) -> str:
    parsed_args = json.loads(arguments or "{}")

    if tool_name == "web_search":
        query = str(parsed_args["query"])
        max_results = int(parsed_args.get("max_results", 5))
        return json.dumps(
            web_search(settings["search_provider"], query, max_results=max_results),
            ensure_ascii=False,
        )

    raise ValueError(f"Unknown tool: {tool_name}")


def run_agent(
    client: OpenAI,
    settings: dict[str, str],
    query: str,
    max_turns: int = 8,
) -> str:
    messages: list[dict[str, Any]] = [
        {"role": "system", "content": SYSTEM_PROMPT},
        {"role": "user", "content": query},
    ]

    for _ in range(max_turns):
        response = client.chat.completions.create(
            model=settings["llm_model"],
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
            tool_output = run_tool(tool_call.function.name, tool_call.function.arguments, settings)
            messages.append(
                {
                    "role": "tool",
                    "tool_call_id": tool_call.id,
                    "content": tool_output,
                }
            )

    raise RuntimeError("Agent exceeded max_turns without producing a final answer")
