from dataclasses import dataclass
from typing import Any


@dataclass
class SearchResult:
    title: str
    url: str
    snippet: str = ""
    score: float | None = None


def build_search_payload(
    provider: str,
    query: str,
    results: list[SearchResult],
    raw: dict[str, Any] | None = None,
) -> dict[str, Any]:
    payload: dict[str, Any] = {
        "provider": provider,
        "query": query,
        "results": [
            {
                "title": result.title,
                "url": result.url,
                "snippet": result.snippet,
                "score": result.score,
            }
            for result in results
        ],
    }
    if raw is not None:
        payload["raw"] = raw
    return payload
