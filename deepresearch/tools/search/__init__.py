from deepresearch.tools.search.base import SearchProvider
from deepresearch.tools.search.tavily import TavilySearchProvider


def get_search_provider(name: str) -> SearchProvider:
    if name == "tavily":
        return TavilySearchProvider()

    raise ValueError(f"Unsupported search provider: {name}")
