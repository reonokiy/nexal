from nexal.tools.base import FunctionTool
from nexal.tools.command import ExecTool
from nexal.tools.edit import EditTool
from nexal.tools.fetch import WebFetchTool
from nexal.tools.final_answer import FinalAnswerTool
from nexal.tools.read import ReadTool
from nexal.tools.search.tavily import WebSearchTool
from nexal.tools.time import TimeTool
from nexal.tools.todo import TodoTool
from nexal.tools.write import WriteTool


def get_default_tools() -> list[FunctionTool]:
    return [
        WebSearchTool(),
        WebFetchTool(),
        TimeTool(),
        ExecTool(),
        ReadTool(),
        EditTool(),
        WriteTool(),
        TodoTool(),
        FinalAnswerTool(),
    ]
