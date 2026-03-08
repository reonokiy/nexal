from nexal.tools.base import FunctionTool
from nexal.tools.command import ExecTool
from nexal.tools.fetch import WebFetchTool
from nexal.tools.search.tavily import WebSearchTool
from nexal.tools.time import TimeTool
from nexal.tools.todo import TodoTool


def get_default_tools() -> list[FunctionTool]:
    return [
        WebSearchTool(),
        WebFetchTool(),
        TimeTool(),
        ExecTool(),
        TodoTool(),
    ]
