"""Run the nexal agent on the BrowseComp benchmark.

Usage:
    uv run python benchmarks/run_browsecomp.py --max-turns 16
    uv run python benchmarks/run_browsecomp.py --limit 10 --max-turns 16  # quick test
    uv run python benchmarks/run_browsecomp.py --resume  # continue from last run
"""

import argparse
import json
import logging
import time
from pathlib import Path

from datasets import load_dataset
from openai import OpenAI

from nexal.agent import run_agent
from nexal.settings import load_settings, settings

logger = logging.getLogger("browsecomp")

OUTPUT_DIR = Path(__file__).resolve().parent / "browsecomp"

JUDGE_PROMPT = """\
You are a judge evaluating whether an agent's answer is correct.

**Question:** {question}

**Reference answer:** {reference}

**Agent's answer:** {agent_answer}

Does the agent's answer contain or match the reference answer? Be lenient with formatting differences, but the core factual content must match.

Respond with exactly one word: CORRECT or INCORRECT"""


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run nexal agent on BrowseComp benchmark.")
    parser.add_argument("--max-turns", type=int, default=16, help="Max agent turns per question (default: 16).")
    parser.add_argument("--limit", type=int, default=0, help="Limit number of questions (0 = all).")
    parser.add_argument("--offset", type=int, default=0, help="Skip first N questions.")
    parser.add_argument("--topic", type=str, default="", help="Filter by problem_topic (e.g. 'Science & technology').")
    parser.add_argument("--output", type=str, default="", help="Output file path (default: auto-generated).")
    parser.add_argument("--resume", action="store_true", help="Resume from existing output file.")
    parser.add_argument("--judge-model", type=str, default="", help="Model for judging (default: same as LLM_MODEL).")
    parser.add_argument("--sleep", type=float, default=2.0, help="Seconds to pause between questions.")
    parser.add_argument("--score-only", action="store_true", help="Only score existing results, don't run agent.")
    return parser.parse_args()


def load_browsecomp(topic: str, offset: int, limit: int) -> list[dict]:
    ds = load_dataset("smolagents/browse_comp", split="test")
    rows = [{"id": i, **row} for i, row in enumerate(ds)]
    if topic:
        rows = [r for r in rows if r.get("problem_topic", "").lower() == topic.lower()]
    rows = rows[offset:]
    if limit > 0:
        rows = rows[:limit]
    return rows


def load_existing(path: Path) -> dict[int, dict]:
    if not path.exists():
        return {}
    results = {}
    for line in path.read_text().splitlines():
        if line.strip():
            row = json.loads(line)
            results[row["id"]] = row
    return results


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as f:
        for row in rows:
            f.write(json.dumps(row, ensure_ascii=False) + "\n")


def judge_answer(client: OpenAI, model: str, question: str, reference: str, agent_answer: str) -> str:
    prompt = JUDGE_PROMPT.format(
        question=question,
        reference=reference,
        agent_answer=agent_answer[:4000],
    )
    try:
        response = client.chat.completions.create(
            model=model,
            messages=[{"role": "user", "content": prompt}],
            max_tokens=10,
        )
        verdict = (response.choices[0].message.content or "").strip().upper()
        return "CORRECT" if "CORRECT" in verdict and "INCORRECT" not in verdict else "INCORRECT"
    except Exception as e:
        logger.warning("judge_error: %s", e)
        return "ERROR"


def score_results(results: list[dict], client: OpenAI, model: str) -> list[dict]:
    for i, row in enumerate(results):
        if row.get("verdict"):
            continue
        verdict = judge_answer(
            client, model,
            question=row["problem"],
            reference=row["answer"],
            agent_answer=row.get("agent_answer", ""),
        )
        row["verdict"] = verdict
        logger.info("[score %d/%d] id=%d verdict=%s", i + 1, len(results), row["id"], verdict)
    return results


def print_summary(results: list[dict]) -> None:
    total = len(results)
    correct = sum(1 for r in results if r.get("verdict") == "CORRECT")
    incorrect = sum(1 for r in results if r.get("verdict") == "INCORRECT")
    errors = sum(1 for r in results if r.get("verdict") == "ERROR")
    no_verdict = total - correct - incorrect - errors

    print(f"\n{'=' * 50}")
    print(f"BrowseComp Results: {correct}/{total} correct ({correct / total * 100:.1f}%)" if total else "No results")
    print(f"  Correct:   {correct}")
    print(f"  Incorrect: {incorrect}")
    if errors:
        print(f"  Errors:    {errors}")
    if no_verdict:
        print(f"  Unscored:  {no_verdict}")

    # Per-topic breakdown
    topics: dict[str, list[dict]] = {}
    for r in results:
        t = r.get("problem_topic", "unknown")
        topics.setdefault(t, []).append(r)
    if len(topics) > 1:
        print(f"\nPer-topic breakdown:")
        for t, rows in sorted(topics.items()):
            tc = sum(1 for r in rows if r.get("verdict") == "CORRECT")
            print(f"  {t}: {tc}/{len(rows)} ({tc / len(rows) * 100:.1f}%)")
    print(f"{'=' * 50}\n")


def main() -> None:
    args = parse_args()
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(name)s %(message)s")

    load_settings()

    judge_model = args.judge_model or settings.llm_model
    client = OpenAI(base_url=settings.llm_api_endpoint, api_key=settings.llm_api_key)

    output_path = Path(args.output) if args.output else OUTPUT_DIR / "results.jsonl"

    if args.score_only:
        existing = load_existing(output_path)
        if not existing:
            print(f"No results found at {output_path}")
            return
        results = list(existing.values())
        results = score_results(results, client, judge_model)
        write_jsonl(output_path, results)
        print_summary(results)
        return

    questions = load_browsecomp(args.topic, args.offset, args.limit)
    existing = load_existing(output_path) if args.resume else {}
    total = len(questions)

    print(f"BrowseComp: {total} questions, max_turns={args.max_turns}")
    if existing:
        print(f"  Resuming: {len(existing)} existing results")

    results_by_id: dict[int, dict] = dict(existing)

    for i, q in enumerate(questions, 1):
        qid = q["id"]
        if qid in results_by_id:
            logger.info("[%d/%d] skip id=%d (already done)", i, total, qid)
            continue

        logger.info("[%d/%d] run id=%d topic=%s", i, total, qid, q.get("problem_topic", ""))
        logger.info("  question: %s", q["problem"][:200])

        try:
            agent_answer = run_agent(q["problem"], max_turns=args.max_turns)
        except KeyboardInterrupt:
            logger.info("interrupted at id=%d, saving progress", qid)
            break
        except Exception as e:
            logger.error("agent_error id=%d: %s", qid, e)
            agent_answer = f"[ERROR] {e}"

        verdict = judge_answer(client, judge_model, q["problem"], q["answer"], agent_answer)
        logger.info("  verdict=%s answer_preview=%s", verdict, agent_answer[:200])

        results_by_id[qid] = {
            "id": qid,
            "problem": q["problem"],
            "problem_topic": q.get("problem_topic", ""),
            "answer": q["answer"],
            "agent_answer": agent_answer,
            "verdict": verdict,
        }

        # Write after each question for crash recovery
        ordered = [results_by_id[qq["id"]] for qq in questions if qq["id"] in results_by_id]
        write_jsonl(output_path, ordered)

        if args.sleep > 0 and i < total:
            time.sleep(args.sleep)

    all_results = [results_by_id[qq["id"]] for qq in questions if qq["id"] in results_by_id]
    write_jsonl(output_path, all_results)
    print_summary(all_results)
    print(f"Results saved to {output_path}")


if __name__ == "__main__":
    main()
