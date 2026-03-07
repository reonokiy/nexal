from dataclasses import dataclass, field
from typing import Protocol


@dataclass
class SandboxMount:
    host_path: str
    container_path: str
    read_only: bool = False


@dataclass
class BaseSandboxConfig:
    image: str | None = None
    workspace_dir: str | None = None
    workspace_read_only: bool = False
    shared_dirs: list[SandboxMount] = field(default_factory=list)
    env: dict[str, str] = field(default_factory=dict)
    network: str = "none"
    memory: str = "512m"
    cpus: float = 1.0
    pids_limit: int = 256


@dataclass
class EphemeralSandboxConfig(BaseSandboxConfig):
    pass


@dataclass
class SandboxConfig(BaseSandboxConfig):
    session_id: str = field(default="")

    def __post_init__(self) -> None:
        if not self.session_id:
            raise ValueError("session_id is required")


@dataclass
class SandboxExecRequest:
    command: str | list[str]
    timeout_seconds: int = 60
    system: bool = False


@dataclass
class EphemeralSandboxResult:
    config: EphemeralSandboxConfig
    request: SandboxExecRequest
    image: str
    exit_code: int
    stdout: str
    stderr: str
    podman_command: str


@dataclass
class SandboxSessionStartResult:
    config: SandboxConfig
    container_name: str
    created: bool
    started: bool
    already_running: bool
    exit_code: int
    stdout: str = ""
    stderr: str = ""
    podman_command: str = ""


@dataclass
class SandboxSessionExecResult:
    request: SandboxExecRequest
    container_name: str
    exit_code: int
    stdout: str
    stderr: str
    podman_command: str


@dataclass
class SandboxSessionStopResult:
    session_id: str
    container_name: str
    exit_code: int
    stdout: str
    stderr: str
    podman_command: str


class EphemeralSandboxBackend(Protocol):
    def exec(self, config: EphemeralSandboxConfig, request: SandboxExecRequest) -> EphemeralSandboxResult:
        pass


class SandboxSession(Protocol):
    session_id: str

    def exec(self, request: SandboxExecRequest) -> SandboxSessionExecResult:
        pass

    def stop(self) -> SandboxSessionStopResult:
        pass


class SandboxManagerBackend(Protocol):
    def start(self, config: SandboxConfig) -> SandboxSession:
        pass
