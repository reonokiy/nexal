import argparse

from agent_core import config, init_client, run_agent


def get_user_query() -> str:
    parser = argparse.ArgumentParser()
    parser.add_argument("--query", required=True, help="Research query")
    args = parser.parse_args()
    return args.query.strip()


def main() -> None:
    query = get_user_query()
    if not query:
        raise SystemExit("Please provide --query.")

    settings = config()
    client = init_client(settings)
    answer = run_agent(client, settings, query)
    print(answer)


if __name__ == "__main__":
    main()
