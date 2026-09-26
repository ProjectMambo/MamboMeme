"""Evaluate ranked retrieval and emit a provisional headless scorecard."""

from __future__ import annotations

import argparse
import json
import math
import os
import platform
import random
import resource
import sqlite3
import sys
import time
from collections import defaultdict
from pathlib import Path
from statistics import mean
from typing import Any, Iterable

import numpy as np

from .retrieve import (
    CANDIDATE_DEPTH,
    DENSE_MINIMUM_SIMILARITY,
    FTS_WEIGHTS,
    RRF_K,
    TEMPLATE_LIMIT,
    RETRIEVER_VERSION,
    SearchEngine,
)


REQUIRED_SLICES = ("entity", "quote", "topic", "semantic", "visual", "noisy")
ROUTES = ("lexical", "dense", "hybrid")
BOOTSTRAP_SEED = 20260925


def ndcg_at_k(ranking: list[str], relevance: dict[str, float], k: int = 10) -> float:
    def dcg(grades: Iterable[float]) -> float:
        return sum((2.0**grade - 1.0) / math.log2(rank + 1) for rank, grade in enumerate(grades, 1))

    ideal = dcg(sorted(relevance.values(), reverse=True)[:k])
    if ideal == 0:
        return 0.0
    return dcg(relevance.get(item_id, 0.0) for item_id in ranking[:k]) / ideal


def exact_reciprocal_rank(ranking: list[str], targets: set[str], k: int = 10) -> float:
    for rank, item_id in enumerate(ranking[:k], 1):
        if item_id in targets:
            return 1.0 / rank
    return 0.0


def _percentile(values: list[float], percentile: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    position = (len(ordered) - 1) * percentile
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    return ordered[lower] * (upper - position) + ordered[upper] * (position - lower)


def _clip(value: float) -> float:
    return min(1.0, max(0.0, value))


def technical_score(metrics: dict[str, Any], p95_ms: float, error_rate: float) -> dict[str, Any]:
    quality = metrics["macro_ndcg_at_10"]
    exact = metrics["exact_mrr_at_10"]
    coverage = metrics["coverage_harmonic"]
    safety = metrics["safe_list_rate"]
    latency = _clip((1000.0 - p95_ms) / 800.0)
    reliability = _clip((0.01 - error_rate) / 0.01)
    raw = 100.0 * (
        0.55 * quality
        + 0.15 * exact
        + 0.10 * coverage
        + 0.10 * safety
        + 0.05 * latency
        + 0.05 * reliability
    )
    failures = []
    failures.extend(
        f"{name} nDCG@10 < 0.30"
        for name, value in metrics["slice_ndcg_at_10"].items()
        if value < 0.30
    )
    for failed, label in (
        (exact < 0.60, "ExactMRR@10 < 0.60"),
        (metrics["hit_at_10"] < 0.70, "Hit@10 < 0.70"),
        (metrics["correct_empty"] < 0.60, "CorrectEmpty < 0.60"),
        (safety < 0.98, "safe-list rate < 0.98"),
        (p95_ms >= 1000.0, "engine p95 >= 1000 ms"),
        (error_rate >= 0.01, "error rate >= 1%"),
    ):
        if failed:
            failures.append(label)
    score = min(raw, 59.0) if failures else raw
    return {
        "components": {
            "coverage": coverage,
            "exact": exact,
            "latency_engine_proxy": latency,
            "quality": quality,
            "reliability": reliability,
            "safety": safety,
        },
        "failed_gates": failures,
        "raw_score": raw,
        "score": score,
        "status": "FAIL" if failures else "PASS",
    }


def _phase2_score_readiness(
    metrics: dict[str, Any], p95_ms: float, error_rate: float
) -> dict[str, Any]:
    diagnostic = technical_score(metrics, p95_ms, error_rate)
    return {
        "in_process_p95_ms": p95_ms,
        "missing_inputs": [
            "human-labelled safety/adversarial subset",
            "Rust TUI submit-to-render latency",
        ],
        "ordinary_safe_list_rate": metrics["safe_list_rate"],
        "retrieval_gate_failures": diagnostic["failed_gates"],
        "score": None,
        "status": "INCOMPLETE",
    }


def load_benchmark(path: Path, serving_ids: set[str]) -> dict[str, Any]:
    benchmark = json.loads(path.read_text("utf-8"))
    if not isinstance(benchmark, dict) or not isinstance(benchmark.get("queries"), list):
        raise ValueError("benchmark must contain a query array")
    if not isinstance(benchmark.get("benchmark_version"), str) or not benchmark["benchmark_version"]:
        raise ValueError("benchmark_version must be a non-empty string")
    queries = benchmark["queries"]
    identifiers = [query.get("id") for query in queries if isinstance(query, dict)]
    if (
        len(identifiers) != len(queries)
        or any(not isinstance(identifier, str) or not identifier for identifier in identifiers)
        or len(set(identifiers)) != len(identifiers)
    ):
        raise ValueError("benchmark query IDs must be unique strings")
    families: dict[str, set[str]] = defaultdict(set)
    relevant_items: dict[str, set[str]] = defaultdict(set)
    observed_counts: dict[str, dict[str, int]] = defaultdict(
        lambda: {"answerable": 0, "no_match": 0}
    )
    slice_counts: dict[str, dict[str, int]] = defaultdict(lambda: defaultdict(int))
    for query in queries:
        partition = query.get("partition")
        answerable = query.get("answerable")
        relevance = query.get("relevance")
        exact_targets = query.get("exact_targets")
        if (
            partition not in {"development", "holdout"}
            or not isinstance(answerable, bool)
            or not isinstance(query.get("cues"), list)
            or not query["cues"]
            or any(not isinstance(cue, str) or not cue for cue in query["cues"])
            or not isinstance(relevance, dict)
            or not isinstance(exact_targets, list)
        ):
            raise ValueError(f"invalid benchmark query: {query.get('id')}")
        if any(item_id not in serving_ids for item_id in relevance):
            raise ValueError(f"unknown relevance item: {query['id']}")
        if any(
            not isinstance(item_id, str)
            or isinstance(grade, bool)
            or not isinstance(grade, (int, float))
            or not 0 <= grade <= 3
            for item_id, grade in relevance.items()
        ):
            raise ValueError(f"invalid relevance grade: {query['id']}")
        if any(not isinstance(target, str) for target in exact_targets):
            raise ValueError(f"invalid exact target: {query['id']}")
        if any(target not in relevance or relevance[target] != 3 for target in exact_targets):
            raise ValueError(f"exact target is not independently grade 3: {query['id']}")
        family = query.get("family")
        if not isinstance(family, str) or not family:
            raise ValueError(f"missing query family: {query['id']}")
        families[partition].add(family)
        relevant_items[partition].update(
            item_id for item_id, grade in relevance.items() if grade > 0
        )
        if answerable:
            if query.get("slice") not in REQUIRED_SLICES or not relevance:
                raise ValueError(f"invalid answerable query: {query['id']}")
            observed_counts[partition]["answerable"] += 1
            slice_counts[partition][query["slice"]] += 1
            if query["slice"] in {"entity", "quote"} and not exact_targets:
                raise ValueError(f"exact query has no exact target: {query['id']}")
        else:
            if relevance or query.get("slice") != "no_match":
                raise ValueError(f"invalid no-match query: {query['id']}")
            observed_counts[partition]["no_match"] += 1
    if families["development"] & families["holdout"]:
        raise ValueError("query families leak across benchmark partitions")
    if relevant_items["development"] & relevant_items["holdout"]:
        raise ValueError("relevant items leak across benchmark partitions")
    if observed_counts != benchmark.get("partitions"):
        raise ValueError("benchmark partition counts do not match metadata")
    for partition in ("development", "holdout"):
        counts = [slice_counts[partition][name] for name in REQUIRED_SLICES]
        if not counts or counts[0] == 0 or len(set(counts)) != 1:
            raise ValueError(f"answerable slices are not balanced: {partition}")
    return benchmark


def _summarize(rows: list[dict[str, Any]]) -> dict[str, Any]:
    answerable = [row for row in rows if row["answerable"]]
    no_match = [row for row in rows if not row["answerable"]]
    slices = {
        name: mean(row["ndcg_at_10"] for row in answerable if row["slice"] == name)
        for name in REQUIRED_SLICES
    }
    exact_rows = [row for row in answerable if row["has_exact_targets"]]
    hit = mean(row["hit_at_10"] for row in answerable)
    correct_empty = mean(row["is_empty"] for row in no_match)
    coverage = 0.0 if hit + correct_empty == 0 else 2 * hit * correct_empty / (hit + correct_empty)
    return {
        "correct_empty": correct_empty,
        "coverage_harmonic": coverage,
        "exact_mrr_at_10": mean(row["exact_rr_at_10"] for row in exact_rows),
        "hit_at_10": hit,
        "macro_ndcg_at_10": mean(slices.values()),
        "query_count": len(rows),
        "safe_list_rate": mean(row["safe_list"] for row in rows),
        "slice_ndcg_at_10": slices,
    }


def _evaluate_route(
    engine: SearchEngine, queries: list[dict[str, Any]], route: str
) -> tuple[list[dict[str, Any]], list[float], int]:
    rows = []
    latencies = []
    errors = 0
    for query in queries:
        started = time.perf_counter_ns()
        failed = False
        try:
            response = engine.search(query["cues"], route=route, limit=10)
            results = response["results"]
        except Exception:
            errors += 1
            failed = True
            results = []
        latencies.append((time.perf_counter_ns() - started) / 1_000_000)
        ranking = [result["id"] for result in results]
        relevance = {item_id: float(grade) for item_id, grade in query["relevance"].items()}
        targets = set(query["exact_targets"])
        rows.append(
            {
                "answerable": query["answerable"],
                "exact_rr_at_10": exact_reciprocal_rank(ranking, targets),
                "family": query["family"],
                "has_exact_targets": bool(targets),
                "hit_at_10": any(relevance.get(item_id, 0) >= 2 for item_id in ranking[:10]),
                "id": query["id"],
                "is_empty": not ranking and not failed,
                "ndcg_at_10": ndcg_at_k(ranking, relevance),
                "ranking": ranking,
                "safe_list": not failed and all(result["safe"] for result in results),
                "slice": query["slice"],
                "error": failed,
            }
        )
    return rows, latencies, errors


def _performance(
    engine: SearchEngine,
    queries: list[dict[str, Any]],
    route: str,
    iterations: int,
) -> tuple[dict[str, Any], int]:
    latencies = []
    errors = 0
    for index in range(100):
        engine.search(queries[index % len(queries)]["cues"], route=route, limit=10)
    schedule = []
    rounds = math.ceil(iterations / len(queries))
    for block in range(rounds):
        shuffled = list(queries)
        random.Random(BOOTSTRAP_SEED + block).shuffle(shuffled)
        schedule.extend(shuffled)
    for query in schedule:
        started = time.perf_counter_ns()
        try:
            engine.search(query["cues"], route=route, limit=10)
        except Exception:
            errors += 1
        latencies.append((time.perf_counter_ns() - started) / 1_000_000)
    return {
        "boundary": "in_process_search",
        "p50_ms": _percentile(latencies, 0.50),
        "p95_ms": _percentile(latencies, 0.95),
        "p99_ms": _percentile(latencies, 0.99),
        "queries": len(schedule),
        "queries_per_second": 1000.0 / mean(latencies) if latencies else 0.0,
    }, errors


def _macro_difference(
    rows: list[tuple[str, float]], required_slices: set[str]
) -> float | None:
    by_slice: dict[str, list[float]] = defaultdict(list)
    for slice_name, difference in rows:
        by_slice[slice_name].append(difference)
    if set(by_slice) != required_slices:
        return None
    return mean(mean(values) for values in by_slice.values())


def _paired_interval(
    baseline: list[dict[str, Any]], candidate: list[dict[str, Any]], samples: int = 2000
) -> tuple[float, float]:
    baseline_by_id = {row["id"]: row for row in baseline if row["answerable"]}
    candidate_by_id = {row["id"]: row for row in candidate if row["answerable"]}
    if set(baseline_by_id) != set(candidate_by_id):
        raise ValueError("paired bootstrap query IDs do not match")
    by_family: dict[str, list[tuple[str, float]]] = defaultdict(list)
    for row in candidate:
        if row["answerable"]:
            baseline_row = baseline_by_id[row["id"]]
            if (row["family"], row["slice"]) != (
                baseline_row["family"],
                baseline_row["slice"],
            ):
                raise ValueError("paired bootstrap family or slice does not match")
            by_family[row["family"]].append(
                (
                    row["slice"],
                    row["ndcg_at_10"] - baseline_row["ndcg_at_10"],
                )
            )
    families = sorted(by_family)
    if not families:
        raise ValueError("paired bootstrap has no answerable queries")
    required_slices = {row["slice"] for row in candidate if row["answerable"]}
    generator = random.Random(BOOTSTRAP_SEED)
    statistics = []
    while len(statistics) < samples:
        sampled = [generator.choice(families) for _ in families]
        statistic = _macro_difference(
            [row for family in sampled for row in by_family[family]], required_slices
        )
        if statistic is not None:
            statistics.append(statistic)
    return _percentile(statistics, 0.025), _percentile(statistics, 0.975)


def evaluate(
    engine: SearchEngine,
    benchmark: dict[str, Any],
    *,
    performance_queries: int = 1000,
) -> dict[str, Any]:
    queries = benchmark["queries"]
    route_rows: dict[str, dict[str, list[dict[str, Any]]]] = {}
    route_reports: dict[str, Any] = {}
    for route in ROUTES:
        rows, evaluation_latencies, evaluation_errors = _evaluate_route(engine, queries, route)
        by_partition = {
            partition: [row for row in rows if query_partition(queries, row["id"]) == partition]
            for partition in ("development", "holdout")
        }
        route_rows[route] = by_partition
        performance, performance_errors = _performance(
            engine,
            [query for query in queries if query["partition"] == "holdout"],
            route,
            performance_queries,
        )
        metrics = {partition: _summarize(partition_rows) for partition, partition_rows in by_partition.items()}
        error_rate = (evaluation_errors + performance_errors) / (
            len(evaluation_latencies) + performance["queries"]
        )
        route_reports[route] = {
            "error_rate": error_rate,
            "metrics": metrics,
            "performance": performance,
            "mmts_search_v1": _phase2_score_readiness(
                metrics["holdout"], performance["p95_ms"], error_rate
            ),
            "query_results": by_partition,
        }

    lexical = route_reports["lexical"]["metrics"]["holdout"]
    hybrid = route_reports["hybrid"]["metrics"]["holdout"]
    overall_delta = hybrid["macro_ndcg_at_10"] - lexical["macro_ndcg_at_10"]
    semantic_delta = hybrid["slice_ndcg_at_10"]["semantic"] - lexical["slice_ndcg_at_10"]["semantic"]
    exact_regression = max(
        lexical["slice_ndcg_at_10"][name] - hybrid["slice_ndcg_at_10"][name]
        for name in ("entity", "quote")
    )
    interval = _paired_interval(route_rows["lexical"]["holdout"], route_rows["hybrid"]["holdout"])
    semantic_accepted = (
        (overall_delta >= 0.03 or semantic_delta >= 0.05)
        and interval[0] > 0
        and exact_regression <= 0.02
        and not route_reports["hybrid"]["mmts_search_v1"]["retrieval_gate_failures"]
    )
    return {
        "benchmark_status": "PROVISIONAL",
        "benchmark_version": benchmark["benchmark_version"],
        "dataset_version": engine.dataset_version,
        "default_route": "hybrid" if semantic_accepted else "lexical",
        "environment": {
            "machine": platform.machine(),
            "numpy": np.__version__,
            "platform": platform.platform(),
            "python": platform.python_version(),
            "sqlite": sqlite3.sqlite_version,
        },
        "limitations": [
            "Repository-visible synthetic fixture judgements are not independent human labels.",
            "In-process search latency excludes NDJSON and UI work; submit-to-render latency begins in Phase 3.",
            "The fixture holdout is not the sealed hidden benchmark defined for a full release.",
            "MMTS-Search-v1 is intentionally unscored until safety-subset and TUI latency inputs exist.",
        ],
        "peak_rss_kib": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,
        "minimum_performance_queries_per_route": performance_queries,
        "retrieval_config": {
            "candidate_depth": CANDIDATE_DEPTH,
            "cue_weights": "equal",
            "dense_minimum_similarity": DENSE_MINIMUM_SIMILARITY,
            "fts_weights": list(FTS_WEIGHTS),
            "rrf_k": RRF_K,
            "route_weights": "equal",
            "template_limit": TEMPLATE_LIMIT,
        },
        "retriever_version": RETRIEVER_VERSION,
        "snapshot_id": engine.manifest["snapshot_id"],
        "storage": {
            name: artifact["bytes"]
            for name, artifact in engine.manifest["artifacts"].items()
        },
        "routes": route_reports,
        "semantic_acceptance": {
            "accepted": semantic_accepted,
            "entity_quote_max_regression": exact_regression,
            "hybrid_minus_lexical_ndcg": overall_delta,
            "paired_bootstrap_95_percent": list(interval),
            "semantic_slice_delta": semantic_delta,
        },
    }


def query_partition(queries: list[dict[str, Any]], query_id: str) -> str:
    return next(query["partition"] for query in queries if query["id"] == query_id)


def _markdown(report: dict[str, Any]) -> str:
    lines = [
        "# Phase 2 provisional scorecard",
        "",
        f"Benchmark: `{report['benchmark_version']}`",
        f"Dataset: `{report['dataset_version']}`",
        f"Default route decision: **{report['default_route']}**",
        "",
        "| Route | nDCG@10 | ExactMRR@10 | Hit@10 | CorrectEmpty | In-process p95 | MMTS | Status |",
        "|---|---:|---:|---:|---:|---:|---:|---|",
    ]
    for route in ROUTES:
        entry = report["routes"][route]
        metrics = entry["metrics"]["holdout"]
        score = entry["mmts_search_v1"]
        lines.append(
            f"| {route} | {metrics['macro_ndcg_at_10']:.3f} | "
            f"{metrics['exact_mrr_at_10']:.3f} | {metrics['hit_at_10']:.3f} | "
            f"{metrics['correct_empty']:.3f} | {entry['performance']['p95_ms']:.3f} ms | "
            f"— | {score['status']} |"
        )
    acceptance = report["semantic_acceptance"]
    lines.extend(
        [
            "",
            "## Semantic decision",
            "",
            f"Dense fusion accepted: **{acceptance['accepted']}**. "
            f"Overall nDCG delta `{acceptance['hybrid_minus_lexical_ndcg']:.3f}`; "
            f"semantic-slice delta `{acceptance['semantic_slice_delta']:.3f}`; "
            f"paired 95% interval `{acceptance['paired_bootstrap_95_percent']}`.",
            "",
            "## Limits",
            "",
            *(f"- {limitation}" for limitation in report["limitations"]),
            "",
        ]
    )
    return "\n".join(lines)


def _write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    with temporary.open("w", encoding="utf-8", newline="\n") as stream:
        stream.write(content)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--data-dir", required=True, type=Path)
    parser.add_argument("--benchmark", required=True, type=Path)
    parser.add_argument("--output-json", required=True, type=Path)
    parser.add_argument("--output-markdown", required=True, type=Path)
    parser.add_argument("--performance-queries", type=int, default=1000)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    if args.performance_queries < 1:
        _parser().error("--performance-queries must be positive")
    started = time.perf_counter_ns()
    engine = SearchEngine(args.data_dir)
    cold_start_ms = (time.perf_counter_ns() - started) / 1_000_000
    with engine:
        benchmark = load_benchmark(args.benchmark, set(engine.items))
        report = evaluate(
            engine,
            benchmark,
            performance_queries=args.performance_queries,
        )
    report["search_engine_startup_ms"] = cold_start_ms
    _write(args.output_json, json.dumps(report, indent=2, sort_keys=True) + "\n")
    _write(args.output_markdown, _markdown(report))
    print(json.dumps({"default_route": report["default_route"], "status": "PROVISIONAL"}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
