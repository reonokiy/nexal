from deepresearch.tools.base import FunctionTool
from deepresearch.tools.command import ExecTool
from deepresearch.tools.fetch import WebFetchTool
from deepresearch.tools.search.tavily import WebSearchTool
from deepresearch.tools.time import TimeTool
from deepresearch.tools.todo import TodoTool


def get_default_tools() -> list[FunctionTool]:
    return [
        WebSearchTool(),
        WebFetchTool(),
        TimeTool(),
        ExecTool(),
        TodoTool(),
    ]
