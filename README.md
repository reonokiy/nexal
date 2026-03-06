# Minimal DeepResearch Agent

This repo contains a minimal deepresearch agent implemented from scratch on top of the OpenAI-compatible Chat Completions API.

## Requirements

- `LLM_API_KEY`
- Optional: `LLM_ENDPOINT` (defaults to `https://openrouter.ai/api/v1`)
- Optional: `LLM_MODEL` (defaults to `openai/gpt-4o`)
- Optional: `SEARCH_PROVIDER` (defaults to `tavily`)
- `TAVILY_API_KEY` when using `tavily`

## Run

```bash
uv run python main.py --query "OpenAI latest developer updates"
```

## Notes

- The agent does not use `openai-agents`.
- It exposes one built-in tool: `web_search`.
- `web_search` uses an abstract provider layer.
- The only supported provider right now is `tavily`.
- Search providers now live under [search/](/home/lean/i/deepresearch/search).

## DeepResearch Bench

To generate benchmark-compatible raw outputs for [benchmarks/deep_research_bench](/home/lean/i/deepresearch/benchmarks/deep_research_bench), run:

```bash
uv run python benchmarks/run_deepresearch_bench.py --model-name my-agent
```

This writes benchmark-compatible rows to:

```text
benchmarks/deep_research_bench/data/test_data/raw_data/my-agent.jsonl
```
