import argparse

from deepresearch.agent import init_client, run_agent
from deepresearch.settings import AgentSettings, load_settings


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--query", required=True, help="Research query")
    parser.add_argument("--session-id", help="Optional sandbox session id to reuse")
    parser.add_argument(
        "--workspace-read-only",
        action="store_true",
        help="Mount the default sandbox /workspace as read-only",
    )
    parser.add_argument(
        "--disable-network",
        action="store_true",
        help="Disable network access for sandbox run_command calls",
    )
    args = parser.parse_args()
    query = args.query.strip()

    settings: AgentSettings = load_settings()
    if args.session_id:
        settings.sandbox_session_id = args.session_id
    if args.workspace_read_only:
        settings.sandbox_workspace_read_only = True
    if args.disable_network:
        settings.sandbox_network_enabled = False
    client = init_client(settings)
    answer = run_agent(client, settings, query)
    print(answer)
