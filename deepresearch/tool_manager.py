from dataclasses import dataclass, field
import logging

from deepresearch.sandbox.base import SandboxSession
from deepresearch.settings import AgentSettings
from deepresearch.tools import AgentTool, ToolContext, get_default_tools


logger = logging.getLogger("deepresearch.agent")


@dataclass
class ToolManager:
    tools: list[AgentTool] = field(default_factory=get_default_tools)

    def __post_init__(self) -> None:
        self._tool_map: dict[str, AgentTool] = {tool.name: tool for tool in self.tools}

    @property
    def openai_tools(self) -> list[dict]:
        return [tool.to_openai_tool() for tool in self.tools]

    def execute(
        self,
        tool_name: str,
        arguments: str,
        settings: AgentSettings,
        session: SandboxSession | None = None,
    ) -> tuple[str, SandboxSession | None]:
        logger.info("tool_call_start name=%s args=%s", tool_name, arguments or "{}")
        tool = self._tool_map.get(tool_name)
        if tool is None:
            raise ValueError(f"Unknown tool: {tool_name}")

        result = tool.execute(arguments, ToolContext(settings=settings, sandbox_session=session))
        logger.info("tool_call_end name=%s output=%s", tool_name, result.output)
        return result.output, result.sandbox_session
