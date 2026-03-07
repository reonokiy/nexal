from dataclasses import dataclass, field
import json
import os
from typing import Any, ClassVar

import httpx

from deepresearch.tools.base import FunctionTool


@dataclass
class WebSearchParams:
    query: str
    max_results: int = 5


@dataclass
class WebSearchTool(FunctionTool):
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
    params_type: ClassVar[type] = WebSearchParams
    api_key: str | None = None

    def execute(self, params: WebSearchParams) -> str:
        api_key = self.api_key or os.getenv("TAVILY_API_KEY")
        if not api_key:
            return json.dumps({"error": "TAVILY_API_KEY environment variable is required"})

        try:
            response = httpx.post(
                "https://api.tavily.com/search",
                json={
                    "api_key": api_key,
                    "query": params.query,
                    "max_results": max(1, min(params.max_results, 10)),
                    "search_depth": "advanced",
                    "include_answer": False,
                    "include_raw_content": False,
                },
                timeout=30,
            )
            response.raise_for_status()
        except httpx.HTTPError as e:
            return json.dumps({"error": str(e)})
        payload = response.json()

        return json.dumps(
            {
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
            },
            ensure_ascii=False,
        )
