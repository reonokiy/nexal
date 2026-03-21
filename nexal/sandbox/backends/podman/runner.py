import os
import shlex
import subprocess
from pathlib import Path
from nexal.sandbox.base import (
    EphemeralSandboxConfig,
    EphemeralSandboxResult,
    SandboxConfig,
    SandboxExecRequest,
    SandboxMount,
)

_DEFAULT_IMAGE = "ghcr.io/reonokiy/nexal-sandbox:python3.13-debian13"


def _workspace_root() -> Path:
    return Path(os.getenv("SANDBOX_WORKSPACE_ROOT", Path.cwd())).resolve()


def _resolve_host_path(raw_path: str, workspace_root: Path | None = None) -> Path:
    if workspace_root is None:
        workspace_root = _workspace_root()
    path = Path(raw_path)
    if not path.is_absolute():
        path = workspace_root / path
    path = path.resolve()

    try:
        path.relative_to(workspace_root)
    except ValueError as exc:
        raise ValueError(f"Mount path must stay within workspace root: {workspace_root}") from exc

    return path


def _build_mount_args(shared_dirs: list[SandboxMount]) -> list[str]:
    if not shared_dirs:
        return []

    workspace_root = _workspace_root()
    args: list[str] = []
    for item in shared_dirs:
        host_path = _resolve_host_path(item.host_path, workspace_root)
        container_path = item.container_path or "/workspace/shared"
        mount_spec = f"type=bind,src={host_path},target={container_path}"
        if item.read_only:
            mount_spec += ",ro=true"
        args.extend(["--mount", mount_spec])

    return args


def _build_workspace_mount_args(workspace_dir: str | None, read_only: bool = False) -> list[str]:
    if not workspace_dir:
        return []

    host_path = Path(workspace_dir).resolve()
    host_path.mkdir(parents=True, exist_ok=True)
    mount_spec = f"type=bind,src={host_path},target=/workspace"
    if read_only:
        mount_spec += ",ro=true"
    return ["--mount", mount_spec]


def _build_env_args(env: dict[str, str] | None) -> list[str]:
    if not env:
        return []

    args: list[str] = []
    for key, value in env.items():
        args.extend(["--env", f"{key}={value}"])
    return args


def _build_runtime_args(runtime: str | None) -> list[str]:
    if not runtime:
        return []
    return ["--runtime", runtime]


def _container_name(session_id: str) -> str:
    if not session_id:
        raise ValueError("session_id must not be empty")
    sanitized = "".join(ch if ch.isalnum() or ch in "-_." else "-" for ch in session_id)
    sanitized = sanitized.strip("-_.")
    if not sanitized:
        raise ValueError(f"session_id produced empty container name: {session_id!r}")
    return f"nexal-sbx-{sanitized}"


_DEFAULT_TIMEOUT = 30


def run_subprocess(args: list[str], timeout_seconds: int | None = _DEFAULT_TIMEOUT) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        capture_output=True,
        text=True,
        errors="replace",
        timeout=timeout_seconds,
        check=False,
    )


def container_exists(container_name: str) -> bool:
    completed = run_subprocess(["podman", "container", "exists", container_name])
    return completed.returncode == 0


def container_running(container_name: str) -> bool:
    completed = run_subprocess(["podman", "inspect", "-f", "{{.State.Running}}", container_name])
    return completed.returncode == 0 and completed.stdout.strip().lower() == "true"


def run_podman_command(config: EphemeralSandboxConfig, request: SandboxExecRequest) -> EphemeralSandboxResult:
    image_name = config.image or os.getenv("SANDBOX_IMAGE", _DEFAULT_IMAGE)
    podman_args = [
        "podman",
        "run",
        "--rm",
        "--userns=keep-id",
        "--security-opt",
        "no-new-privileges",
        "--cap-drop=ALL",
        "--pids-limit",
        str(config.pids_limit),
        "--memory",
        config.memory,
        "--cpus",
        str(config.cpus),
        "--network",
        config.network,
        "--workdir",
        "/workspace",
    ]
    podman_args.extend(
        _build_workspace_mount_args(config.workspace_dir, read_only=config.workspace_read_only)
    )
    podman_args.extend(_build_mount_args(config.shared_dirs))
    podman_args.extend(_build_env_args(config.env))
    podman_args.extend(_build_runtime_args(config.runtime))
    podman_args.append(image_name)
    if isinstance(request.command, str):
        podman_args.extend(["bash", "-c", request.command])
    else:
        podman_args.extend(request.command)

    completed = run_subprocess(podman_args, timeout_seconds=request.timeout_seconds)
    return EphemeralSandboxResult(
        config=config,
        request=request,
        image=image_name,
        exit_code=completed.returncode,
        stdout=completed.stdout,
        stderr=completed.stderr,
        podman_command=shlex.join(podman_args),
    )


def container_name(session_id: str) -> str:
    return _container_name(session_id)


def build_create_args(config: SandboxConfig) -> list[str]:
    image_name = config.image or os.getenv("SANDBOX_IMAGE", _DEFAULT_IMAGE)
    args = [
        "podman",
        "create",
        "--name",
        _container_name(config.session_id),
        "--userns=keep-id",
        "--security-opt",
        "no-new-privileges",
        "--cap-drop=ALL",
        "--pids-limit",
        str(config.pids_limit),
        "--memory",
        config.memory,
        "--cpus",
        str(config.cpus),
        "--network",
        config.network,
        "--workdir",
        "/workspace",
    ]
    args.extend(_build_workspace_mount_args(config.workspace_dir, read_only=config.workspace_read_only))
    args.extend(_build_mount_args(config.shared_dirs))
    args.extend(_build_env_args(config.env))
    args.extend(_build_runtime_args(config.runtime))
    args.extend([image_name, "sleep", "infinity"])
    return args


def build_stop_args(session_id: str) -> list[str]:
    return ["podman", "rm", "-f", _container_name(session_id)]
