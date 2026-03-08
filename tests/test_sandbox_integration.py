import shutil
import tempfile
from collections.abc import Generator
from pathlib import Path
from uuid6 import uuid7

import pytest

from nexal.sandbox import (
    EphemeralSandbox,
    EphemeralSandboxConfig,
    Sandbox,
    SandboxConfig,
    SandboxExecRequest,
)


TEST_ROOT = Path(".tmp_test_sandbox")


@pytest.fixture(scope="session", autouse=True)
def test_root() -> Generator[Path]:
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

                timeout_seconds=30,
            )
        )
    finally:
        sandbox.stop()

    assert write_result.exit_code == 0, write_result.stderr
    assert read_result.exit_code == 0, read_result.stderr
    assert "persisted" in read_result.stdout
    assert (workspace / "state.txt").read_text(encoding="utf-8") == "persisted"


@pytest.mark.integration
def test_persistent_sandbox_preserves_bash_state(workspace: Path) -> None:
    """Verify that env vars and cwd persist across exec calls via state file."""
    session_id = f"test-{uuid7()}"
    sandbox = Sandbox(
        config=SandboxConfig(
            session_id=session_id,
            workspace_dir=str(workspace),
            network="none",
        )
    )

    try:
        # Set an env var and change directory.
        r1 = sandbox.exec(SandboxExecRequest(
            command="export MY_VAR=hello123 && mkdir -p /workspace/subdir && cd /workspace/subdir",
            timeout_seconds=10,
        ))
        assert r1.exit_code == 0, r1.stderr

        # Verify env var and cwd persisted.
        r2 = sandbox.exec(SandboxExecRequest(
            command="echo $MY_VAR && pwd",
            timeout_seconds=10,
        ))
        assert r2.exit_code == 0, r2.stderr
        assert "hello123" in r2.stdout
        assert "/workspace/subdir" in r2.stdout

        # Override env var, verify it sticks.
        r3 = sandbox.exec(SandboxExecRequest(
            command="export MY_VAR=updated",
            timeout_seconds=10,
        ))
        assert r3.exit_code == 0, r3.stderr

        r4 = sandbox.exec(SandboxExecRequest(
            command="echo $MY_VAR",
            timeout_seconds=10,
        ))
        assert r4.exit_code == 0, r4.stderr
        assert "updated" in r4.stdout
    finally:
        sandbox.stop()


@pytest.mark.integration
def test_agents_dir_readonly_for_exec_commands(workspace: Path) -> None:
    """/workspace/agents/ is read-only inside exec commands, writable from host."""
    session_id = f"test-{uuid7()}"
    sandbox = Sandbox(
        config=SandboxConfig(
            session_id=session_id,
            workspace_dir=str(workspace),
            network="none",
        )
    )

    try:
        # Write a file from host side (simulating system write via workspace.py).
        agents_dir = workspace / "agents"
        agents_dir.mkdir(parents=True, exist_ok=True)
        (agents_dir / "test.txt").write_text("secret", encoding="utf-8")

        # Exec command: cannot write to /workspace/agents/.
        r1 = sandbox.exec(SandboxExecRequest(
            command="echo hacked > /workspace/agents/test.txt",
            timeout_seconds=10,
        ))
        assert r1.exit_code != 0

        # Exec command: can still read /workspace/agents/.
        r2 = sandbox.exec(SandboxExecRequest(
            command="cat /workspace/agents/test.txt",
            timeout_seconds=10,
        ))
        assert r2.exit_code == 0, r2.stderr
        assert "secret" in r2.stdout

        # Verify state file still works after the failed write.
        r3 = sandbox.exec(SandboxExecRequest(
            command="export AFTER_FAIL=yes",
            timeout_seconds=10,
        ))
        assert r3.exit_code == 0, r3.stderr

        r4 = sandbox.exec(SandboxExecRequest(
            command="echo $AFTER_FAIL",
            timeout_seconds=10,
        ))
        assert r4.exit_code == 0, r4.stderr
        assert "yes" in r4.stdout
    finally:
        sandbox.stop()
