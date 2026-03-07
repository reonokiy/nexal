from dataclasses import dataclass
import shlex

from deepresearch.sandbox.backends.podman.runner import (
    build_create_args,
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

_STATE_FILE = "/workspace/agents/.bash_state"
_AGENT_DIR = "/workspace/agents"


class PodmanEphemeralSandbox(EphemeralSandboxBackend):
    def exec(self, config: EphemeralSandboxConfig, request: SandboxExecRequest) -> EphemeralSandboxResult:
        return run_podman_command(config, request)


@dataclass
class PodmanSandboxSession(SandboxSession):
    session_id: str
    start_result: SandboxSessionStartResult | None = None

    def exec(self, request: SandboxExecRequest) -> SandboxSessionExecResult:
        name = container_name(self.session_id)
        cmd_str = request.command if isinstance(request.command, str) else shlex.join(request.command)

        # Wrapper script: restore state, protect agents dir, run command, save state.
        parts: list[str] = [
            f"[ -f {_STATE_FILE} ] && . {_STATE_FILE}",
            f"chmod -R a-w {_AGENT_DIR} 2>/dev/null",
            cmd_str,
            "__rc=$?",
            f"chmod -R u+w {_AGENT_DIR} 2>/dev/null",
            f"mkdir -p {_AGENT_DIR}",
            f'{{ export -p; echo "cd \\"$(pwd)\\""; }} > {_STATE_FILE}',
            "exit $__rc",
        ]

        script = "\n".join(parts)
        exec_args = [
            "podman", "exec",
            name,
            "bash", "-c", script,
        ]

        completed = run_subprocess(exec_args, timeout_seconds=request.timeout_seconds)
        return SandboxSessionExecResult(
            request=request,
            container_name=name,
            exit_code=completed.returncode,
            stdout=completed.stdout,
            stderr=completed.stderr,
            podman_command=cmd_str,
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

        # Try to create; ignore "already exists" errors (avoids TOCTOU race).
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
        if created.returncode == 0:
            created_now = True
        elif "already" not in (created.stderr + created.stdout).lower():
            raise RuntimeError(
                f"Failed to create sandbox {name}: {created.stderr.strip() or created.stdout.strip()}"
            )

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
                created=created_now,
                started=True,
                already_running=not created_now,
                exit_code=0,
            ),
        )
