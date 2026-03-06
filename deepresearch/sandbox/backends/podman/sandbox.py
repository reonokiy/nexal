from dataclasses import dataclass
import shlex

from deepresearch.sandbox.backends.podman.runner import (
    build_create_args,
    build_exec_args,
    build_stop_args,
    container_exists,
    container_name,
    container_running,
    run_podman_command,
    run_subprocess,
)
from deepresearch.sandbox.base import (
    EphemeralSandboxConfig,
    EphemeralSandboxBackend,
    EphemeralSandboxResult,
    SandboxConfig,
    SandboxExecRequest,
    SandboxManagerBackend,
    SandboxSession,
    SandboxSessionExecResult,
    SandboxSessionStartResult,
    SandboxSessionStopResult,
)


class PodmanEphemeralSandbox(EphemeralSandboxBackend):
    def exec(self, config: EphemeralSandboxConfig, request: SandboxExecRequest) -> EphemeralSandboxResult:
        return run_podman_command(config, request)


@dataclass
class PodmanSandboxSession(SandboxSession):
    session_id: str
    start_result: SandboxSessionStartResult | None = None

    def exec(self, request: SandboxExecRequest) -> SandboxSessionExecResult:
        if not request.command:
            raise ValueError("command must not be empty")

        exec_args = build_exec_args(self.session_id, request)
        completed = run_subprocess(exec_args, timeout_seconds=request.timeout_seconds)
        return SandboxSessionExecResult(
            request=request,
            container_name=container_name(self.session_id),
            exit_code=completed.returncode,
            stdout=completed.stdout,
            stderr=completed.stderr,
            podman_command=shlex.join(exec_args),
        )

    def stop(self) -> SandboxSessionStopResult:
        stop_args = build_stop_args(self.session_id)
        completed = run_subprocess(stop_args)
        return SandboxSessionStopResult(
            session_id=self.session_id,
            container_name=container_name(self.session_id),
            exit_code=completed.returncode,
            stdout=completed.stdout,
            stderr=completed.stderr,
            podman_command=shlex.join(stop_args),
        )


class PodmanSandboxManager(SandboxManagerBackend):
    def start(self, config: SandboxConfig) -> SandboxSession:
        name = container_name(config.session_id)
        created_now = False
        if not container_exists(name):
            create_args = build_create_args(
                config.session_id,
                image=config.image,
                workspace_dir=config.workspace_dir,
                workspace_read_only=config.workspace_read_only,
                shared_dirs=config.shared_dirs,
                env=config.env,
                network=config.network,
                memory=config.memory,
                cpus=config.cpus,
                pids_limit=config.pids_limit,
            )
            created = run_subprocess(create_args)
            if created.returncode != 0:
                raise RuntimeError(
                    f"Failed to create sandbox {name}: {created.stderr.strip() or created.stdout.strip()}"
                )
            created_now = True

        if not container_running(name):
            start_args = ["podman", "start", name]
            started = run_subprocess(start_args)
            if started.returncode != 0:
                raise RuntimeError(
                    f"Failed to start sandbox {name}: {started.stderr.strip() or started.stdout.strip()}"
                )

        return PodmanSandboxSession(
            session_id=config.session_id,
            start_result=SandboxSessionStartResult(
                config=config,
                container_name=name,
                created=True,
                started=True,
                already_running=not created_now,
                exit_code=0,
            ),
        )
