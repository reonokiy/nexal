# nexal

A minimal AI agent framework built on the OpenAI-compatible Chat Completions API with sandboxed tool execution.

## Requirements

- `LLM_API_KEY`
- Optional: `LLM_ENDPOINT` (defaults to `https://openrouter.ai/api/v1`)
- Optional: `LLM_MODEL` (defaults to `openai/gpt-4o`)
- `TAVILY_API_KEY`

## Run

```bash
uv run nexal --query "OpenAI latest developer updates"
```

Or run the package directly:

```bash
uv run python -m nexal --query "OpenAI latest developer updates"
```

## Architecture

- ReAct loop (Thought → Action → Observation) with configurable max turns.
- Built-in tools: `web_search`, `web_fetch`, `exec`, `todo`, `get_current_datetime`.
- `web_search` uses Tavily. Search providers live under `nexal/tools/search/`.
- `exec` runs commands in a rootless Podman container with persistent bash state.
- Sandbox abstraction in `nexal/sandbox/base.py`, default backend under `nexal/sandbox/backends/podman/`.
- Each agent run gets a UUID-based host directory mounted at `/workspace`.
- Reuse a sandbox session with `--session-id` or `SANDBOX_SESSION_ID`.
- `/workspace` mount can be made read-only globally or per `exec` call.
- Network is enabled by default; disable with `--disable-network` or `SANDBOX_NETWORK_ENABLED=false`.

## Benchmark

To generate benchmark-compatible raw outputs for `benchmarks/deep_research_bench/`:

```bash
uv run python benchmarks/run_deepresearch_bench.py --model-name my-agent
```

Output:

```text
benchmarks/deep_research_bench/data/test_data/raw_data/my-agent.jsonl
```
