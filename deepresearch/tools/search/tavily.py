from dataclasses import dataclass, field, fields
import json
import os
from typing import Any
from urllib.request import Request, urlopen

from deepresearch.settings import AgentSettings
from deepresearch.tools.base import FunctionTool


@dataclass
class WebSearchParams:
    query: str
    max_results: int = 5


@dataclass
class TavilyWebSearch(FunctionTool):
    name: str = "web_search"
    description: str = "Search the web and return a short list of relevant results."
    parameters: dict[str, Any] = field(
        default_factory=lambda: {
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
        init=False,
    )
    api_key: str | None = None

    def execute(self, arguments: str, settings: AgentSettings) -> str:
        parsed = json.loads(arguments or "{}")
        valid = {f.name for f in fields(WebSearchParams)}
        params = WebSearchParams(**{k: v for k, v in parsed.items() if k in valid})
        return json.dumps(self._search(params), ensure_ascii=False)

    def _search(self, params: WebSearchParams) -> dict[str, Any]:
        api_key = self.api_key or os.getenv("TAVILY_API_KEY")
        if not api_key:
            raise RuntimeError("TAVILY_API_KEY environment variable is required")

        body = json.dumps(
            {
                "api_key": api_key,
                "query": params.query,
                "max_results": max(1, min(params.max_results, 10)),
                "search_depth": "advanced",
                "include_answer": False,
                "include_raw_content": False,
            }
        ).encode("utf-8")
        request = Request(
            "https://api.tavily.com/search",
            data=body,
            headers={"Content-Type": "application/json", "Accept": "application/json"},
            method="POST",
        )

        with urlopen(request, timeout=30) as response:
            payload = json.loads(response.read().decode("utf-8"))

        return {
            "provider": "tavily",
            "query": params.query,
            "results": [
                {
                    "title": item.get("title", ""),
                    "url": item.get("url", ""),
                    "snippet": item.get("content", ""),
                    "score": item.get("score"),
                }
                for item in payload.get("results", [])
                if item.get("url")
            ],
        }
