from dataclasses import dataclass, field

from nexal.sandbox.base import (
    EphemeralSandboxBackend,
    EphemeralSandboxConfig,
    EphemeralSandboxResult,
    SandboxConfig,
    SandboxExecRequest,
    SandboxManagerBackend,
    SandboxSession,
    SandboxSessionExecResult,
    SandboxSessionStopResult,
)
from nexal.sandbox.backends.podman.sandbox import PodmanEphemeralSandbox, PodmanSandboxManager


@dataclass
class EphemeralSandbox:
    config: EphemeralSandboxConfig = field(default_factory=EphemeralSandboxConfig)
    backend: EphemeralSandboxBackend = field(default_factory=PodmanEphemeralSandbox)

    def exec(self, request: SandboxExecRequest) -> EphemeralSandboxResult:
        return self.backend.exec(self.config, request)


@dataclass
class Sandbox:
    session_id: str | None = None
    config: SandboxConfig | None = None
    manager: SandboxManagerBackend = field(default_factory=PodmanSandboxManager)
    _session: SandboxSession | None = field(default=None, init=False, repr=False)

    def start(self) -> SandboxSession:
        if self._session is not None:
            return self._session

        if self.config is None:
            if not self.session_id:
                raise ValueError("session_id or config is required for persistent sandbox")
            self.config = SandboxConfig(session_id=self.session_id)

        self._session = self.manager.start(self.config)
        return self._session

    def exec(self, request: SandboxExecRequest) -> SandboxSessionExecResult:
        return self.start().exec(request)

    def stop(self) -> SandboxSessionStopResult | None:
        if self._session is None:
            return None
        result = self._session.stop()
        self._session = None
        return result
