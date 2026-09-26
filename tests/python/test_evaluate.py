from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "python"))

from mambomeme_search.evaluate import (  # noqa: E402
    _evaluate_route,
    _paired_interval,
    exact_reciprocal_rank,
    load_benchmark,
    ndcg_at_k,
    technical_score,
)


class EvaluationTest(unittest.TestCase):
    def test_route_error_is_not_a_correct_or_safe_empty_result(self) -> None:
        class FailingEngine:
            def search(self, *_: object, **__: object) -> dict[str, object]:
                raise RuntimeError("boom")

        rows, _, errors = _evaluate_route(
            FailingEngine(),
            [
                {
                    "answerable": False,
                    "cues": ["missing"],
                    "exact_targets": [],
                    "family": "missing-family",
                    "id": "missing",
                    "relevance": {},
                    "slice": "no_match",
                }
            ],
            "lexical",
        )
        self.assertEqual(errors, 1)
        self.assertFalse(rows[0]["is_empty"])
        self.assertFalse(rows[0]["safe_list"])

    def test_ndcg_and_exact_reciprocal_rank(self) -> None:
        relevance = {"a": 3, "b": 2, "c": 1}
        self.assertEqual(ndcg_at_k(["a", "b", "c"], relevance), 1.0)
        self.assertLess(ndcg_at_k(["c", "b", "a"], relevance), 1.0)
        self.assertEqual(ndcg_at_k([], relevance), 0.0)
        self.assertEqual(ndcg_at_k(["x"], {}), 0.0)
        self.assertEqual(exact_reciprocal_rank(["x", "a"], {"a"}), 0.5)
        self.assertEqual(exact_reciprocal_rank(["x"], {"a"}), 0.0)

    def test_technical_score_bounds_and_gate_cap(self) -> None:
        metrics = {
            "correct_empty": 1.0,
            "coverage_harmonic": 1.0,
            "exact_mrr_at_10": 1.0,
            "hit_at_10": 1.0,
            "macro_ndcg_at_10": 1.0,
            "safe_list_rate": 1.0,
            "slice_ndcg_at_10": {
                name: 1.0
                for name in ("entity", "quote", "topic", "semantic", "visual", "noisy")
            },
        }
        passing = technical_score(metrics, 200.0, 0.0)
        self.assertEqual((passing["score"], passing["status"]), (100.0, "PASS"))

        metrics["slice_ndcg_at_10"]["semantic"] = 0.0
        metrics["macro_ndcg_at_10"] = 5 / 6
        failing = technical_score(metrics, 1000.0, 0.01)
        self.assertEqual(failing["score"], 59.0)
        self.assertEqual(failing["status"], "FAIL")
        self.assertGreaterEqual(len(failing["failed_gates"]), 3)

    def test_paired_bootstrap_is_deterministic(self) -> None:
        baseline = [
            {
                "id": f"q{index}",
                "family": f"family-{index // 2}",
                "answerable": True,
                "ndcg_at_10": 0.25,
                "slice": "entity" if index < 2 else "semantic",
            }
            for index in range(4)
        ]
        candidate = [
            {
                "id": f"q{index}",
                "family": f"family-{index // 2}",
                "answerable": True,
                "ndcg_at_10": 0.75,
                "slice": "entity" if index < 2 else "semantic",
            }
            for index in range(4)
        ]
        self.assertEqual(
            _paired_interval(baseline, candidate, samples=100),
            _paired_interval(baseline, candidate, samples=100),
        )
        self.assertGreater(_paired_interval(baseline, candidate, samples=100)[0], 0)

    def test_fixture_benchmark_contract_and_family_leak(self) -> None:
        path = ROOT / "benchmarks/provisional-v1.json"
        benchmark = json.loads(path.read_text("utf-8"))
        serving_ids = {
            item_id
            for query in benchmark["queries"]
            for item_id in query["relevance"]
        }
        loaded = load_benchmark(path, serving_ids)
        self.assertEqual(loaded["benchmark_version"], "fixture-provisional-v1")

        without_version = json.loads(path.read_text("utf-8"))
        without_version.pop("benchmark_version")
        with tempfile.TemporaryDirectory() as temporary:
            invalid = Path(temporary) / "invalid.json"
            invalid.write_text(json.dumps(without_version), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "benchmark_version"):
                load_benchmark(invalid, serving_ids)

        benchmark["queries"][-1]["family"] = benchmark["queries"][0]["family"]
        with tempfile.TemporaryDirectory() as temporary:
            invalid = Path(temporary) / "invalid.json"
            invalid.write_text(json.dumps(benchmark), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "leak"):
                load_benchmark(invalid, serving_ids)

        benchmark = json.loads(path.read_text("utf-8"))
        holdout = next(query for query in benchmark["queries"] if query["partition"] == "holdout")
        development_item = next(
            item_id
            for query in benchmark["queries"]
            if query["partition"] == "development"
            for item_id in query["relevance"]
        )
        holdout["relevance"][development_item] = 1
        with tempfile.TemporaryDirectory() as temporary:
            invalid = Path(temporary) / "invalid.json"
            invalid.write_text(json.dumps(benchmark), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "relevant items leak"):
                load_benchmark(invalid, serving_ids)


if __name__ == "__main__":
    unittest.main()
