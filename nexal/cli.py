import argparse

from nexal.agent import run_agent
from nexal.settings import settings, load_settings


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
        "--enable-sandbox-network",
        action="store_true",
        help="Enable network access for sandbox exec calls (default: disabled)",
    )
    parser.add_argument(
        "--max-turns", type=int, default=10,
        help="Maximum agent turns (default: 10)",
    )
    args = parser.parse_args()
    task = args.task.strip()

    load_settings()
    if args.session:
        settings.sandbox_session_id = args.session
    if args.workspace_readonly:
        settings.sandbox_workspace_read_only = True
    if args.enable_sandbox_network:
        settings.sandbox_network_enabled = True
    answer = run_agent(task, max_turns=args.max_turns)
    print(answer)
