import json
import os
from urllib.request import Request, urlopen

from search.base import SearchResult, build_search_payload


class TavilySearchProvider:
    name = "tavily"

    def __init__(self, api_key: str | None = None) -> None:
        self.api_key = api_key or os.getenv("TAVILY_API_KEY")
        if not self.api_key:
            raise RuntimeError("TAVILY_API_KEY environment variable is required for SEARCH_PROVIDER=tavily")

    def search(self, query: str, max_results: int = 5) -> dict[str, object]:
        safe_max_results = max(1, min(max_results, 10))
        body = json.dumps(
            {
                "api_key": self.api_key,
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
        return build_search_payload(self.name, query, results)
