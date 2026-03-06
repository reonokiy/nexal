from search.base import SearchProvider
from search.tavily import TavilySearchProvider


def get_search_provider(name: str) -> SearchProvider:
    if name == "tavily":
        return TavilySearchProvider()

    raise ValueError(f"Unsupported search provider: {name}")
