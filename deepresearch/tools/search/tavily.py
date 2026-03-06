from dataclasses import dataclass, field
import json
import os
from typing import Any
from urllib.request import Request, urlopen

from deepresearch.tools.base import FunctionTool, ToolContext, ToolExecutionResult
from deepresearch.tools.search.base import SearchResult, build_search_payload


def _tavily_web_search_parameters() -> dict[str, Any]:
    return {
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
    }


@dataclass(frozen=True)
class TavilyWebSearch(FunctionTool):
    name: str = "web_search"
    description: str = "Search the web and return a short list of relevant results."
    parameters: dict[str, Any] = field(default_factory=_tavily_web_search_parameters, init=False)
    provider_name: str = field(default="tavily", init=False)
    api_key: str | None = None

    def execute(self, arguments: str, context: ToolContext) -> ToolExecutionResult:
        parsed_args = json.loads(arguments or "{}")
        query = str(parsed_args["query"])
        max_results = int(parsed_args.get("max_results", 5))
        output = json.dumps(self.search(query, max_results=max_results), ensure_ascii=False)
        return ToolExecutionResult(output=output, sandbox_session=context.sandbox_session)

    def search(self, query: str, max_results: int = 5) -> dict[str, object]:
        api_key = self.api_key or os.getenv("TAVILY_API_KEY")
        if not api_key:
            raise RuntimeError("TAVILY_API_KEY environment variable is required")

        safe_max_results = max(1, min(max_results, 10))
        body = json.dumps(
            {
                "api_key": api_key,
                "query": query,
                "max_results": safe_max_results,
                "search_depth": "advanced",
                "include_answer": False,
                "include_raw_content": False,
            }
        ).encode("utf-8")
        request = Request(
            "https://api.tavily.com/search",
            data=body,
            headers={
                "Content-Type": "application/json",
                "Accept": "application/json",
            },
            method="POST",
        )

        with urlopen(request, timeout=30) as response:
            payload = json.loads(response.read().decode("utf-8"))

        results = [
            SearchResult(
                title=item.get("title", ""),
                url=item.get("url", ""),
                snippet=item.get("content", ""),
                score=item.get("score"),
            )
            for item in payload.get("results", [])
            if item.get("url")
        ]
        return build_search_payload(self.provider_name, query, results)
