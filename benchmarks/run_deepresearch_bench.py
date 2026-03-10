"""Run the nexal agent on the DeepResearch Bench benchmark.

Usage:
    uv run python benchmarks/run_deepresearch_bench.py --model-name gpt-4o --max-turns 16
    uv run python benchmarks/run_deepresearch_bench.py --model-name gpt-4o --limit 5  # quick test
    uv run python benchmarks/run_deepresearch_bench.py --model-name gpt-4o --parallel 4 --sandbox-network
"""

import argparse
import json
import logging
import time
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
from nexal.agent import run_agent
from nexal.settings import load_settings, settings

logger = logging.getLogger("deepresearch_bench")

DEFAULT_BENCH_ROOT = ROOT / "benchmarks" / "deep_research_bench"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run the local deep research agent on DeepResearch Bench queries.")
    parser.add_argument("--model-name", required=True, help="Output model name used by deep_research_bench.")
    parser.add_argument(
        "--bench-root",
        default=str(DEFAULT_BENCH_ROOT),
        help="Path to the deep_research_bench checkout.",
    )
    parser.add_argument(
        "--query-file",
        default="data/prompt_data/query.jsonl",
        help="Path to benchmark query jsonl, relative to --bench-root.",
    )
    parser.add_argument(
        "--output-dir",
        default="data/test_data/raw_data",
        help="Directory for raw benchmark outputs, relative to --bench-root.",
    )
    parser.add_argument("--limit", type=int, default=0, help="Optional limit for quick testing.")
    parser.add_argument("--only-zh", action="store_true", help="Only run Chinese prompts.")
    parser.add_argument("--only-en", action="store_true", help="Only run English prompts.")
    parser.add_argument("--force", action="store_true", help="Overwrite existing output file.")
    parser.add_argument("--max-turns", type=int, default=8, help="Maximum agent turns per query.")
    parser.add_argument("--sleep-seconds", type=float, default=0.0, help="Optional pause between queries.")
    parser.add_argument("--sandbox-network", action="store_true", help="Enable sandbox network access.")
    parser.add_argument("--parallel", type=int, default=1, help="Number of parallel workers (default: 1).")
    return parser.parse_args()


def load_jsonl(path: Path) -> list[dict]:
    with path.open("r", encoding="utf-8") as handle:
        return [json.loads(line) for line in handle if line.strip()]


def load_existing_results(path: Path) -> dict[int, dict]:
    if not path.exists():
        return {}

    rows = load_jsonl(path)
    existing: dict[int, dict] = {}
    for row in rows:
        item_id = row.get("id")
        if isinstance(item_id, int):
            existing[item_id] = row
    return existing


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, ensure_ascii=False) + "\n")


def filter_queries(rows: list[dict], only_zh: bool, only_en: bool, limit: int) -> list[dict]:
    if only_zh and only_en:
        raise ValueError("Cannot set both --only-zh and --only-en")

    filtered = rows
    if only_zh:
        filtered = [row for row in filtered if row.get("language") == "zh"]
    if only_en:
        filtered = [row for row in filtered if row.get("language") == "en"]
    if limit > 0:
        filtered = filtered[:limit]
    return filtered


def _run_one(row: dict, max_turns: int, sandbox_network: bool) -> dict:
    """Worker function for parallel execution. Runs in a subprocess."""
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(name)s %(message)s")
    load_settings()
    settings.sandbox_network_enabled = sandbox_network
    # Reset session so each worker gets its own sandbox
    settings.sandbox_session_id = ""
    settings.sandbox_workspace_dir = ""

    item_id = row["id"]
    prompt = row["prompt"]
    logger.info("run id=%d", item_id)

    try:
        article = run_agent(prompt, max_turns=max_turns)
    except Exception as e:
        logger.error("agent_error id=%d: %s", item_id, e)
        article = f"[ERROR] {e}"

    return {
        "id": item_id,
        "prompt": prompt,
        "article": article,
    }


def main() -> None:
    args = parse_args()
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(name)s %(message)s")

    bench_root = Path(args.bench_root).resolve()
    query_path = bench_root / args.query_file
    output_path = (bench_root / args.output_dir / f"{args.model_name}.jsonl").resolve()

    queries = filter_queries(load_jsonl(query_path), args.only_zh, args.only_en, args.limit)
    existing = {} if args.force else load_existing_results(output_path)

    load_settings()
    settings.sandbox_network_enabled = args.sandbox_network

    results_by_id: dict[int, dict] = dict(existing)
    pending = [row for row in queries if row["id"] not in results_by_id]
    total = len(queries)

    print(f"DeepResearch Bench: {total} queries, {len(pending)} pending, max_turns={args.max_turns}, parallel={args.parallel}")
    if existing:
        print(f"  Resuming: {len(existing)} existing results")

    if args.parallel > 1:
        # --- Parallel execution ---
        try:
            with ProcessPoolExecutor(max_workers=args.parallel) as executor:
                futures = {
                    executor.submit(_run_one, row, args.max_turns, args.sandbox_network): row
                    for row in pending
                }
                for fut in as_completed(futures):
                    row = futures[fut]
                    item_id = row["id"]
                    try:
                        result = fut.result()
                    except Exception as e:
                        logger.error("worker_error id=%d: %s", item_id, e)
                        result = {
                            "id": item_id,
                            "prompt": row["prompt"],
                            "article": f"[ERROR] {e}",
                        }

                    results_by_id[item_id] = result
                    logger.info("done id=%d article_len=%d", item_id, len(result["article"]))

                    # Write after each completion for crash recovery
                    ordered = [results_by_id[q["id"]] for q in queries if q["id"] in results_by_id]
                    write_jsonl(output_path, ordered)
        except KeyboardInterrupt:
            logger.info("interrupted, saving progress")
    else:
        # --- Sequential execution ---
        for index, row in enumerate(pending, start=1):
            item_id = row["id"]
            prompt = row["prompt"]

            logger.info("[%d/%d] run id=%d", index, len(pending), item_id)

            try:
                article = run_agent(prompt, max_turns=args.max_turns)
            except KeyboardInterrupt:
                logger.info("interrupted at id=%d, saving progress", item_id)
                break
            except Exception as e:
                logger.error("agent_error id=%d: %s", item_id, e)
                article = f"[ERROR] {e}"

            results_by_id[item_id] = {
                "id": item_id,
                "prompt": prompt,
                "article": article,
            }
            ordered = [results_by_id[q["id"]] for q in queries if q["id"] in results_by_id]
            write_jsonl(output_path, ordered)

            if args.sleep_seconds > 0 and index < len(pending):
                time.sleep(args.sleep_seconds)

    ordered = [results_by_id[q["id"]] for q in queries if q["id"] in results_by_id]
    write_jsonl(output_path, ordered)
    print(f"Wrote {len(ordered)} rows to {output_path}")


if __name__ == "__main__":
    main()
