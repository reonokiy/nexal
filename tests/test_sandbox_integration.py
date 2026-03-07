import shutil
import tempfile
from pathlib import Path
from uuid import uuid7

import pytest

from deepresearch.sandbox import (
    EphemeralSandbox,
    EphemeralSandboxConfig,
    Sandbox,
    SandboxConfig,
    SandboxExecRequest,
)


TEST_ROOT = Path(".tmp_test_sandbox")


@pytest.fixture(scope="session", autouse=True)
def test_root() -> Path:
    TEST_ROOT.mkdir(exist_ok=True)
    yield TEST_ROOT
    shutil.rmtree(TEST_ROOT, ignore_errors=True)


@pytest.fixture
def workspace(test_root: Path) -> Path:
    return Path(tempfile.mkdtemp(prefix="workspace-", dir=test_root))


@pytest.mark.integration
def test_ephemeral_sandbox_executes_code_and_writes_workspace(workspace: Path) -> None:
    sandbox = EphemeralSandbox(
        config=EphemeralSandboxConfig(
            workspace_dir=str(workspace),
            network="none",
        )
    )

    result = sandbox.exec(
        SandboxExecRequest(
            command=[
                "python",
                "-c",
                (
                    "from pathlib import Path; "
                    "Path('/workspace/out.txt').write_text('ok', encoding='utf-8'); "
                    "print('hello from ephemeral')"
                ),
            ],
            workdir="/workspace",
            timeout_seconds=30,
        )
    )

    assert result.exit_code == 0, result.stderr
    assert "hello from ephemeral" in result.stdout
    assert (workspace / "out.txt").read_text(encoding="utf-8") == "ok"


@pytest.mark.integration
def test_persistent_sandbox_preserves_workspace_between_execs(workspace: Path) -> None:
    session_id = f"test-{uuid7()}"
    sandbox = Sandbox(
        config=SandboxConfig(
            session_id=session_id,
            workspace_dir=str(workspace),
            network="none",
        )
    )

    try:
        write_result = sandbox.exec(
            SandboxExecRequest(
                command=[
                    "python",
                    "-c",
                    (
                        "from pathlib import Path; "
                        "Path('/workspace/state.txt').write_text('persisted', encoding='utf-8'); "
                        "print('write ok')"
                    ),
                ],
                workdir="/workspace",
                timeout_seconds=30,
            )
        )
        read_result = sandbox.exec(
            SandboxExecRequest(
                command=[
                    "python",
                    "-c",
                    (
                        "from pathlib import Path; "
                        "print(Path('/workspace/state.txt').read_text(encoding='utf-8'))"
                    ),
                ],
                workdir="/workspace",
                timeout_seconds=30,
            )
        )
    finally:
        sandbox.stop()

    assert write_result.exit_code == 0, write_result.stderr
    assert read_result.exit_code == 0, read_result.stderr
    assert "persisted" in read_result.stdout
    assert (workspace / "state.txt").read_text(encoding="utf-8") == "persisted"
