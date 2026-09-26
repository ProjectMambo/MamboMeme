"""Offline Phase 1 check: ingest, rebuild, publish, and search the fixture."""

from __future__ import annotations

import json
import os
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "tests" / "fixtures" / "corpus" / "manifest.jsonl"


def run(*command: str, env: dict[str, str] | None = None) -> str:
    return subprocess.run(
        command,
        cwd=ROOT,
        env=env,
        check=True,
        capture_output=True,
        text=True,
    ).stdout


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="mambomeme-phase1-") as temporary:
        data_dir = Path(temporary)
        command = (
            "cargo",
            "run",
            "--quiet",
            "--",
            "ingest",
            "--manifest",
            str(MANIFEST),
            "--data-dir",
            str(data_dir),
        )
        first = json.loads(run(*command))
        second = json.loads(run(*command))
        assert first == {
            "accepted": 14,
            "unchanged": 0,
            "duplicate": 1,
            "quarantined": 1,
            "deleted": 0,
            "fatal_batch": 0,
        }
        assert second == {
            "accepted": 0,
            "unchanged": 15,
            "duplicate": 0,
            "quarantined": 1,
            "deleted": 0,
            "fatal_batch": 0,
        }

        environment = os.environ.copy()
        environment["PYTHONPATH"] = str(ROOT / "python")
        manifest = json.loads(
            run(
                sys.executable,
                "-m",
                "mambomeme_search.build_index",
                "--data-dir",
                str(data_dir),
                env=environment,
            )
        )
        assert manifest["item_count"] == 14

        pointer = json.loads((data_dir / "active.json").read_text(encoding="utf-8"))
        database = data_dir / Path(pointer["manifest"]).parent / "corpus.sqlite"
        with sqlite3.connect(database) as connection:
            results = connection.execute(
                "SELECT item_id FROM meme_fts WHERE meme_fts MATCH 'john cena'"
            ).fetchall()
        assert len(results) == 1 and results[0][0].startswith("mm_")

    print("Phase 1 fixture passed")


if __name__ == "__main__":
    main()
