# Minimal DeepResearch Agent

This repo contains a minimal deepresearch agent implemented from scratch on top of the OpenAI-compatible Chat Completions API. The implementation now lives under the `deepresearch/` package.

## Requirements

- `LLM_API_KEY`
- Optional: `LLM_ENDPOINT` (defaults to `https://openrouter.ai/api/v1`)
- Optional: `LLM_MODEL` (defaults to `openai/gpt-4o`)
- Optional: `SEARCH_PROVIDER` (defaults to `tavily`)
- `TAVILY_API_KEY` when using `tavily`

## Run

```bash
uv run deepresearch --query "OpenAI latest developer updates"
```

You can also run the package directly:

```bash
uv run python -m deepresearch --query "OpenAI latest developer updates"
```

## Notes

- The agent does not use `openai-agents`.
- It exposes three built-in tools: `web_search`, `get_current_datetime`, and `run_command`.
- `web_search` uses an abstract provider layer.
- The only supported provider right now is `tavily`.
- Search providers now live under `deepresearch/tools/search/`.
- `run_command` uses rootless Podman and supports dynamic commands plus bind-mounted shared directories.
- The LLM-facing `run_command` tool uses a persistent stateful container by default.
- A one-off ephemeral sandbox interface is still available at the code layer in `deepresearch/sandbox/backends/podman/runner.py`.
- The sandbox abstraction now lives in `deepresearch/sandbox/base.py`.
- The default backend is implemented under `deepresearch/sandbox/backends/podman/`, and wired through `deepresearch/sandbox/service.py`.
- Each agent run gets its own UUID-based persistent host directory mounted into the container at `/workspace`.
- You can reuse a sandbox session with `--session-id` or `SANDBOX_SESSION_ID`.
- The `/workspace` mount can be made read-only globally or per `run_command` call.
- Network access for the default sandbox can be enabled with `--allow-network` or `SANDBOX_NETWORK_ENABLED=true`.

## DeepResearch Bench

To generate benchmark-compatible raw outputs for `benchmarks/deep_research_bench/`, run:

```bash
uv run python benchmarks/run_deepresearch_bench.py --model-name my-agent
```

This writes benchmark-compatible rows to:

```text
benchmarks/deep_research_bench/data/test_data/raw_data/my-agent.jsonl
```
