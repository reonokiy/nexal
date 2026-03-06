from deepresearch.tools.base import AgentTool
from deepresearch.tools.command import RunCommandTool
from deepresearch.tools.search.tavily import TavilyWebSearch
from deepresearch.tools.time import CurrentDatetimeTool


def get_default_tools() -> list[AgentTool]:
    return [
        TavilyWebSearch(),
        CurrentDatetimeTool(),
        RunCommandTool(),
    ]
