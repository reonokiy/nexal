import argparse

from deepresearch.agent_core import AgentSettings, config, init_client, run_agent


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
        "--allow-network",
        action="store_true",
        help="Allow network access for sandbox run_command calls by default",
    )
    args = parser.parse_args()
    query = args.query.strip()

    settings: AgentSettings = config()
    if args.session_id:
        settings.sandbox_session_id = args.session_id
    if args.workspace_read_only:
        settings.sandbox_workspace_read_only = True
    if args.allow_network:
        settings.sandbox_network_enabled = True
    client = init_client(settings)
    answer = run_agent(client, settings, query)
    print(answer)
