import argparse
import json
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
from deepresearch.agent_core import init_client, run_agent
from deepresearch.settings import load_settings


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


def main() -> None:
    args = parse_args()
    bench_root = Path(args.bench_root).resolve()
    query_path = bench_root / args.query_file
    output_path = (bench_root / args.output_dir / f"{args.model_name}.jsonl").resolve()

    queries = filter_queries(load_jsonl(query_path), args.only_zh, args.only_en, args.limit)
    existing = {} if args.force else load_existing_results(output_path)

    settings = load_settings()
    client = init_client(settings)

    results_by_id: dict[int, dict] = dict(existing)
    total = len(queries)

    for index, row in enumerate(queries, start=1):
        item_id = row["id"]
        prompt = row["prompt"]

        if item_id in results_by_id:
            print(f"[{index}/{total}] skip id={item_id}")
            continue

        print(f"[{index}/{total}] run id={item_id}")
        article = run_agent(client, settings, prompt, max_turns=args.max_turns)
        results_by_id[item_id] = {
            "id": item_id,
            "prompt": prompt,
            "article": article,
        }
        ordered = [results_by_id[q["id"]] for q in queries if q["id"] in results_by_id]
        write_jsonl(output_path, ordered)

        if args.sleep_seconds > 0:
            time.sleep(args.sleep_seconds)

    ordered = [results_by_id[q["id"]] for q in queries if q["id"] in results_by_id]
    write_jsonl(output_path, ordered)
    print(f"Wrote {len(ordered)} rows to {output_path}")


if __name__ == "__main__":
    main()
