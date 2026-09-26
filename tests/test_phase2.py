"""Offline Phase 2 check: corpus, retrieval, worker, and scorecard."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "python"))

from mambomeme_search.build_index import build_snapshot  # noqa: E402
from mambomeme_search.evaluate import evaluate, load_benchmark  # noqa: E402
from mambomeme_search.retrieve import SearchEngine  # noqa: E402


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="mambomeme-phase2-") as temporary:
        data_dir = Path(temporary)
        subprocess.run(
            [
                "cargo",
                "run",
                "--quiet",
                "--",
                "ingest",
                "--manifest",
                str(ROOT / "tests/fixtures/corpus/manifest.jsonl"),
                "--data-dir",
                str(data_dir),
            ],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        )
        build_snapshot(data_dir)

        with SearchEngine(data_dir) as engine:
            entity = engine.search("john cena")
            assert entity["results"][0]["title"] == "John Cena — You Can't See Me"
            benchmark = load_benchmark(
                ROOT / "benchmarks/provisional-v1.json", set(engine.items)
            )
            report = evaluate(engine, benchmark, performance_queries=96)
        assert report["benchmark_status"] == "PROVISIONAL"
        assert report["default_route"] == "lexical"
        assert report["routes"]["lexical"]["metrics"]["holdout"]["hit_at_10"] == 1

        environment = os.environ.copy()
        environment["PYTHONPATH"] = str(ROOT / "python")
        worker = subprocess.Popen(
            [
                sys.executable,
                "-m",
                "mambomeme_search.worker",
                "--data-dir",
                str(data_dir),
            ],
            cwd=ROOT,
            env=environment,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        assert worker.stdin is not None and worker.stdout is not None
        assert json.loads(worker.stdout.readline())["type"] == "ready"
        worker.stdin.write(
            '{"cues":["john cena"],"protocol_version":1,"request_id":"e2e","type":"search"}\n'
        )
        worker.stdin.flush()
        results = json.loads(worker.stdout.readline())
        assert results["type"] == "results" and results["results"]
        worker.stdin.write(
            '{"protocol_version":1,"request_id":"stop","type":"shutdown"}\n'
        )
        worker.stdin.flush()
        assert json.loads(worker.stdout.readline())["type"] == "bye"
        assert worker.wait(timeout=5) == 0
        worker.stdin.close()
        worker.stdout.close()
        assert worker.stderr is not None and worker.stderr.read() == ""
        worker.stderr.close()

    print("Phase 2 fixture passed")


if __name__ == "__main__":
    main()
