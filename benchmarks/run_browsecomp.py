"""Run the nexal agent on the BrowseComp benchmark.

Usage:
    uv run python benchmarks/run_browsecomp.py --max-turns 16
    uv run python benchmarks/run_browsecomp.py --limit 10 --max-turns 16  # quick test
    uv run python benchmarks/run_browsecomp.py --resume  # continue from last run
    uv run python benchmarks/run_browsecomp.py --parallel 4 --sandbox-network  # parallel
"""

import argparse
import base64
import hashlib
import json
import logging
import random
import re
import time
from concurrent.futures import ProcessPoolExecutor, as_completed
from typing import Any
from pathlib import Path

import pandas
import litellm

from nexal.agent import run_agent
from nexal.settings import load_settings, settings

logger = logging.getLogger("browsecomp")

OUTPUT_DIR = Path(__file__).resolve().parent / "browsecomp"
DATASET_URL = "https://openaipublic.blob.core.windows.net/simple-evals/browse_comp_test_set.csv"

# From: https://github.com/openai/simple-evals/blob/main/browsecomp_eval.py
GRADER_TEMPLATE = """
Judge whether the following [response] to [question] is correct or not based on the precise and unambiguous [correct_answer] below.

[question]: {question}

[response]: {response}

Your judgement must be in the format and criteria specified below:

extracted_final_answer: The final exact answer extracted from the [response]. Put the extracted answer as 'None' if there is no exact, final answer to extract from the response.

[correct_answer]: {correct_answer}

reasoning: Explain why the extracted_final_answer is correct or incorrect based on [correct_answer], focusing only on if there are meaningful differences between [correct_answer] and the extracted_final_answer. Do not comment on any background to the problem, do not attempt to solve the problem, do not argue for any answer different than [correct_answer], focus only on whether the answers match.

correct: Answer 'yes' if extracted_final_answer matches the [correct_answer] given above, or is within a small margin of error for numerical problems. Answer 'no' otherwise, i.e. if there if there is any inconsistency, ambiguity, non-equivalency, or if the extracted answer is incorrect.

confidence: The extracted confidence score between 0% and 100% from [response]. Put 100 if there is no confidence score available.
""".strip()


def _derive_key(password: str, length: int) -> bytes:
    key = hashlib.sha256(password.encode()).digest()
    return key * (length // len(key)) + key[: length % len(key)]


def _decrypt(ciphertext_b64: str, password: str) -> str:
    encrypted = base64.b64decode(ciphertext_b64)
    key = _derive_key(password, len(encrypted))
    return bytes(a ^ b for a, b in zip(encrypted, key)).decode()


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
    parser.add_argument("--sample", type=int, default=0, help="Randomly sample N questions (0 = no sampling).")
    parser.add_argument("--seed", type=int, default=42, help="Random seed for sampling (default: 42).")
    parser.add_argument("--sandbox-network", action="store_true", help="Enable sandbox network access.")
    parser.add_argument("--parallel", type=int, default=1, help="Number of parallel workers (default: 1).")
    return parser.parse_args()


def load_browsecomp(topic: str, offset: int, limit: int, sample: int = 0, seed: int = 42) -> list[dict]:
    df = pandas.read_csv(DATASET_URL)
    rows = []
    for i, (_, row) in enumerate(df.iterrows()):
        canary = row.get("canary", "")
        problem = _decrypt(row["problem"], canary) if canary else row["problem"]
        answer = _decrypt(row["answer"], canary) if canary else row["answer"]
        rows.append({
            "id": i,
            "problem": problem,
            "answer": answer,
            "problem_topic": row.get("category", row.get("problem_topic", "")),
        })
    if topic:
        rows = [r for r in rows if str(r.get("problem_topic", "")).lower() == topic.lower()]
    rows = rows[offset:]
    if sample > 0:
        rng = random.Random(seed)
        rows = rng.sample(rows, min(sample, len(rows)))
    elif limit > 0:
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


def _judge_llm_kwargs() -> dict[str, Any]:
    kwargs: dict[str, Any] = {"timeout": 300.0}
    if settings.llm_api_key:
        kwargs["api_key"] = settings.llm_api_key
    if settings.llm_api_base:
        kwargs["api_base"] = settings.llm_api_base
    return kwargs


def judge_answer(model: str, question: str, reference: str, agent_answer: str) -> str:
    """Grade using the official BrowseComp grader template. Returns 'CORRECT' or 'INCORRECT'."""
    prompt = GRADER_TEMPLATE.format(
        question=question,
        correct_answer=reference,
        response=agent_answer[:8000],
    )
    try:
        response = litellm.completion(
            model=model,
            messages=[{"role": "user", "content": prompt}],
            max_tokens=512,
            **_judge_llm_kwargs(),
        )
        grading = response.choices[0].message.content or ""
        match = re.search(r"correct:\s*(yes|no)", grading, re.IGNORECASE)
        verdict = match.group(1).lower() if match else "no"
        return "CORRECT" if verdict == "yes" else "INCORRECT"
    except Exception as e:
        logger.warning("judge_error: %s", e)
        return "ERROR"


def score_results(results: list[dict], model: str) -> list[dict]:
    for i, row in enumerate(results):
        if row.get("verdict"):
            continue
        verdict = judge_answer(
            model,
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
        print("\nPer-topic breakdown:")
        for t, rows in sorted(topics.items()):
            tc = sum(1 for r in rows if r.get("verdict") == "CORRECT")
            print(f"  {t}: {tc}/{len(rows)} ({tc / len(rows) * 100:.1f}%)")
    print(f"{'=' * 50}\n")


def _run_one(question: dict, max_turns: int, sandbox_network: bool) -> dict:
    """Worker function for parallel execution. Runs in a subprocess."""
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(name)s %(message)s")
    load_settings()
    settings.sandbox_network_enabled = sandbox_network
    # Reset session so each worker gets its own sandbox
    settings.sandbox_session_id = ""
    settings.sandbox_workspace_dir = ""

    qid = question["id"]
    logger.info("run id=%d topic=%s", qid, question.get("problem_topic", ""))

    try:
        agent_answer = run_agent(question["problem"], max_turns=max_turns)
    except Exception as e:
        logger.error("agent_error id=%d: %s", qid, e)
        agent_answer = f"[ERROR] {e}"

    return {
        "id": qid,
        "problem": question["problem"],
        "problem_topic": question.get("problem_topic", ""),
        "answer": question["answer"],
        "agent_answer": agent_answer,
    }


def main() -> None:
    args = parse_args()
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(name)s %(message)s")

    load_settings()
    settings.sandbox_network_enabled = args.sandbox_network

    judge_model = args.judge_model or settings.llm_model

    output_path = Path(args.output) if args.output else OUTPUT_DIR / "results.jsonl"

    if args.score_only:
        existing = load_existing(output_path)
        if not existing:
            print(f"No results found at {output_path}")
            return
        results = list(existing.values())
        results = score_results(results, judge_model)
        write_jsonl(output_path, results)
        print_summary(results)
        return

    questions = load_browsecomp(args.topic, args.offset, args.limit, args.sample, args.seed)
    existing = load_existing(output_path) if args.resume else {}
    total = len(questions)

    print(f"BrowseComp: {total} questions, max_turns={args.max_turns}, parallel={args.parallel}")
    if existing:
        print(f"  Resuming: {len(existing)} existing results")

    results_by_id: dict[int, dict] = dict(existing)
    pending = [q for q in questions if q["id"] not in results_by_id]

    if args.parallel > 1:
        # --- Parallel execution ---
        try:
            with ProcessPoolExecutor(max_workers=args.parallel) as executor:
                futures = {
                    executor.submit(_run_one, q, args.max_turns, args.sandbox_network): q
                    for q in pending
                }
                for fut in as_completed(futures):
                    q = futures[fut]
                    qid = q["id"]
                    try:
                        result = fut.result()
                    except Exception as e:
                        logger.error("worker_error id=%d: %s", qid, e)
                        result = {
                            "id": qid,
                            "problem": q["problem"],
                            "problem_topic": q.get("problem_topic", ""),
                            "answer": q["answer"],
                            "agent_answer": f"[ERROR] {e}",
                        }

                    verdict = judge_answer(judge_model, q["problem"], q["answer"], result["agent_answer"])
                    logger.info("id=%d verdict=%s answer_preview=%s", qid, verdict, result["agent_answer"][:200])
                    result["verdict"] = verdict
                    results_by_id[qid] = result

                    # Write after each completion for crash recovery
                    ordered = [results_by_id[qq["id"]] for qq in questions if qq["id"] in results_by_id]
                    write_jsonl(output_path, ordered)
        except KeyboardInterrupt:
            logger.info("interrupted, saving progress")
    else:
        # --- Sequential execution ---
        for i, q in enumerate(pending, 1):
            qid = q["id"]
            logger.info("[%d/%d] run id=%d topic=%s", i, len(pending), qid, q.get("problem_topic", ""))
            logger.info("  question: %s", q["problem"][:200])

            try:
                agent_answer = run_agent(q["problem"], max_turns=args.max_turns)
            except KeyboardInterrupt:
                logger.info("interrupted at id=%d, saving progress", qid)
                break
            except Exception as e:
                logger.error("agent_error id=%d: %s", qid, e)
                agent_answer = f"[ERROR] {e}"

            verdict = judge_answer(judge_model, q["problem"], q["answer"], agent_answer)
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

            if args.sleep > 0 and i < len(pending):
                time.sleep(args.sleep)

    all_results = [results_by_id[qq["id"]] for qq in questions if qq["id"] in results_by_id]
    write_jsonl(output_path, all_results)
    print_summary(all_results)
    print(f"Results saved to {output_path}")


if __name__ == "__main__":
    main()
