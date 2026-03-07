import argparse

from deepresearch.agent import run_agent
from deepresearch.settings import settings, load_settings


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--task", required=True, help="Research task")
    parser.add_argument("--session", help="Optional sandbox session id to reuse")
    parser.add_argument(
        "--workspace-readonly",
        action="store_true",
        help="Mount the default sandbox /workspace as read-only",
    )
    parser.add_argument(
        "--disable-sandbox-network",
        action="store_true",
        help="Disable network access for sandbox exec calls",
    )
    args = parser.parse_args()
    task = args.task.strip()

    load_settings()
    if args.session:
        settings.sandbox_session_id = args.session
    if args.workspace_readonly:
        settings.sandbox_workspace_read_only = True
    if args.disable_sandbox_network:
        settings.sandbox_network_enabled = False
    answer = run_agent(task)
    print(answer)
