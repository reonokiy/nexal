from deepresearch.tools.base import FunctionTool
from deepresearch.tools.command import RunCommandTool
from deepresearch.tools.search.tavily import TavilyWebSearch
from deepresearch.tools.time import CurrentDatetimeTool


def get_default_tools() -> list[FunctionTool]:
    return [
        TavilyWebSearch(),
        CurrentDatetimeTool(),
        RunCommandTool(),
    ]
