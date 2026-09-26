from __future__ import annotations

import hashlib
import json
import os
import selectors
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "python"))

from mambomeme_search.build_index import build_snapshot  # noqa: E402
from mambomeme_search.retrieve import QueryError, SearchEngine  # noqa: E402
from mambomeme_search.worker import _encode, _fit_results, _search_request  # noqa: E402


class RetrievalTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.temporary = tempfile.TemporaryDirectory()
        cls.data_dir = Path(cls.temporary.name)
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
                str(cls.data_dir),
            ],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        )
        build_snapshot(cls.data_dir)

    @classmethod
    def tearDownClass(cls) -> None:
        cls.temporary.cleanup()

    def setUp(self) -> None:
        self.engine = SearchEngine(self.data_dir)

    def tearDown(self) -> None:
        self.engine.close()

    def test_entity_quote_and_literal_punctuation_search(self) -> None:
        entity = self.engine.search("john cena")
        quote = self.engine.search("you can't see me")
        operators = self.engine.search('john OR (cena) - "*"')

        self.assertEqual(entity["results"][0]["title"], "John Cena — You Can't See Me")
        self.assertIn("people", entity["results"][0]["matched_fields"])
        self.assertEqual(quote["results"][0]["id"], entity["results"][0]["id"])
        self.assertEqual(operators["results"][0]["id"], entity["results"][0]["id"])

    def test_filters_multiple_cues_empty_and_deterministic_results(self) -> None:
        first = self.engine.search(["john cena", "invisible"], limit=3)
        second = self.engine.search(["john cena", "invisible"], limit=3)
        deduplicated = self.engine.search(["john cena", "john cena"], limit=3)
        images = self.engine.search("invisible", kind="image")
        wrong_language = self.engine.search("john cena", language="fr")
        missing = self.engine.search("quokka tax audit zxqv")

        self.assertEqual(first, second)
        self.assertEqual(first["results"][0]["title"], "John Cena — You Can't See Me")
        self.assertEqual(deduplicated, self.engine.search("john cena", limit=3))
        self.assertTrue(images["results"])
        self.assertTrue(all(result["kind"] == "image" for result in images["results"]))
        self.assertEqual(wrong_language["results"], [])
        self.assertEqual(missing["results"], [])

    def test_template_cap_refills_from_deeper_candidates(self) -> None:
        item_ids = list(self.engine.items)[:4]
        for item_id in item_ids[:3]:
            self.engine.items[item_id]["template"] = "same-template"
        candidates = [(item_id, float(len(item_ids) - rank)) for rank, item_id in enumerate(item_ids)]
        with mock.patch.object(self.engine, "_lexical", return_value=candidates):
            results = self.engine.search("fixture", limit=4)["results"]
        self.assertEqual([result["id"] for result in results], item_ids[:2] + item_ids[3:])

    def test_dense_hybrid_and_explicit_degradation(self) -> None:
        dense = self.engine.search("raised hand blue figure", route="dense")
        hybrid = self.engine.search("raised hand blue figure", route="hybrid")
        self.assertTrue(dense["results"])
        self.assertTrue(hybrid["results"])

        with mock.patch.object(self.engine.dense, "search", side_effect=RuntimeError("boom")):
            degraded = self.engine.search("john cena", route="hybrid")
        self.assertEqual(degraded["degraded_routes"], ["dense"])
        self.assertTrue(degraded["results"])

    def test_query_validation(self) -> None:
        for cues, keyword, code in [
            ("", {}, "invalid_cue"),
            (["x"] * 5, {}, "invalid_cues"),
            ("john", {"limit": 0}, "invalid_limit"),
            ("john", {"kind": "video"}, "invalid_kind"),
            ("john", {"kind": []}, "invalid_kind"),
            ("john", {"language": "en_US!"}, "invalid_language"),
            ("john", {"route": "magic"}, "invalid_route"),
            ("john", {"route": []}, "invalid_route"),
        ]:
            with self.subTest(code=code), self.assertRaises(QueryError) as raised:
                self.engine.search(cues, **keyword)
            self.assertEqual(raised.exception.code, code)

    def test_corrupt_artifact_fails_startup(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            copy = Path(temporary) / "data"
            shutil.copytree(self.data_dir, copy)
            pointer = json.loads((copy / "active.json").read_text("utf-8"))
            artifact = copy / Path(pointer["manifest"]).parent / "dense_vectors.npy"
            artifact.write_bytes(b"corrupt")
            with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                SearchEngine(copy)

    def test_missing_required_artifact_and_version_fail_startup(self) -> None:
        for mutate, expected in (
            (lambda manifest: manifest["artifacts"].pop("dense_vectors.npy"), "required set"),
            (
                lambda manifest: manifest["artifacts"].update(
                    {"extra.bin": {"bytes": 0, "sha256": "0" * 64}}
                ),
                "required set",
            ),
            (lambda manifest: manifest.update(schema_version=2), "schema_version"),
        ):
            with self.subTest(expected=expected), tempfile.TemporaryDirectory() as temporary:
                copy = Path(temporary) / "data"
                shutil.copytree(self.data_dir, copy)
                pointer = json.loads((copy / "active.json").read_text("utf-8"))
                manifest_path = copy / pointer["manifest"]
                manifest = json.loads(manifest_path.read_text("utf-8"))
                mutate(manifest)
                unsigned = {key: value for key, value in manifest.items() if key != "snapshot_id"}
                manifest["snapshot_id"] = hashlib.sha256(
                    json.dumps(unsigned, sort_keys=True, separators=(",", ":")).encode() + b"\n"
                ).hexdigest()
                manifest_path.write_text(
                    json.dumps(manifest, sort_keys=True, separators=(",", ":")) + "\n",
                    encoding="utf-8",
                )
                snapshot = manifest_path.parent
                renamed = snapshot.parent / manifest["snapshot_id"]
                snapshot.rename(renamed)
                pointer["manifest"] = str(
                    (renamed / "manifest.json").relative_to(copy)
                )
                pointer["snapshot_id"] = manifest["snapshot_id"]
                (copy / "active.json").write_text(
                    json.dumps(pointer, sort_keys=True, separators=(",", ":")) + "\n",
                    encoding="utf-8",
                )
                with self.assertRaisesRegex(ValueError, expected):
                    SearchEngine(copy)

    def test_golden_protocol_messages_decode_in_python(self) -> None:
        messages = [
            json.loads(line)
            for line in (ROOT / "tests/fixtures/protocol/messages.jsonl")
            .read_text("utf-8")
            .splitlines()
        ]
        self.assertEqual(
            [message["type"] for message in messages],
            ["ready", "search", "results", "error", "shutdown", "bye"],
        )
        self.assertEqual(_search_request(messages[1])["kind"], "text")

    def test_worker_round_trip_and_recoverable_error(self) -> None:
        environment = os.environ.copy()
        environment["PYTHONPATH"] = str(ROOT / "python")
        environment["PYTHONIOENCODING"] = "ascii"
        worker = subprocess.Popen(
            [
                sys.executable,
                "-m",
                "mambomeme_search.worker",
                "--data-dir",
                str(self.data_dir),
            ],
            cwd=ROOT,
            env=environment,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        assert worker.stdin is not None and worker.stdout is not None
        ready = json.loads(worker.stdout.readline())
        self.assertEqual(ready["type"], "ready")

        worker.stdin.write(
            json.dumps(
                {
                    "protocol_version": 1,
                    "type": "search",
                    "request_id": "bad",
                    "cues": [],
                }
            )
            + "\n"
        )
        worker.stdin.flush()
        error = json.loads(worker.stdout.readline())
        self.assertEqual((error["type"], error["code"], error["fatal"]), ("error", "invalid_cues", False))

        worker.stdin.write(
            json.dumps(
                {
                    "protocol_version": 1,
                    "type": "search",
                    "request_id": "search-1",
                    "cues": ["john cena"],
                    "limit": 3,
                    "filters": {},
                    "route": "lexical",
                }
            )
            + "\n"
        )
        worker.stdin.flush()
        results = json.loads(worker.stdout.readline())
        self.assertEqual(results["type"], "results")
        self.assertEqual(results["request_id"], "search-1")
        self.assertFalse(results["truncated"])
        self.assertTrue(results["results"])

        worker.stdin.write(
            json.dumps(
                {
                    "protocol_version": 1,
                    "type": "shutdown",
                    "request_id": "stop",
                }
            )
            + "\n"
        )
        worker.stdin.flush()
        self.assertEqual(json.loads(worker.stdout.readline())["type"], "bye")
        self.assertEqual(worker.wait(timeout=5), 0)
        worker.stdin.close()
        worker.stdout.close()
        assert worker.stderr is not None
        self.assertEqual(worker.stderr.read(), "")
        worker.stderr.close()

    def test_worker_protocol_guards(self) -> None:
        environment = os.environ.copy()
        environment["PYTHONPATH"] = str(ROOT / "python")
        command = [
            sys.executable,
            "-m",
            "mambomeme_search.worker",
            "--data-dir",
            str(self.data_dir),
        ]
        search = {
            "protocol_version": 1,
            "type": "search",
            "request_id": "same",
            "cues": ["john cena"],
        }
        mismatch = {
            "protocol_version": True,
            "type": "search",
            "request_id": "wrong-version",
            "cues": ["john cena"],
        }
        completed = subprocess.run(
            command,
            cwd=ROOT,
            env=environment,
            input="9" * 5000
            + "\n"
            + json.dumps(search)
            + "\n"
            + json.dumps(search)
            + "\n"
            + json.dumps(mismatch)
            + "\n",
            capture_output=True,
            text=True,
            timeout=5,
        )
        messages = [json.loads(line) for line in completed.stdout.splitlines()]
        self.assertEqual(completed.returncode, 1)
        self.assertEqual(
            [(message["type"], message.get("code")) for message in messages],
            [
                ("ready", None),
                ("error", "invalid_json"),
                ("results", None),
                ("error", "duplicate_request_id"),
                ("error", "protocol_mismatch"),
            ],
        )
        self.assertEqual(completed.stderr, "")

        for payload, code in (
            ("", "premature_eof"),
            (json.dumps({**search, "request_id": "partial"}), "partial_line"),
            ("x" * (64 * 1024 + 1) + "\n", "message_too_large"),
        ):
            with self.subTest(code=code):
                completed = subprocess.run(
                    command,
                    cwd=ROOT,
                    env=environment,
                    input=payload,
                    capture_output=True,
                    text=True,
                    timeout=5,
                )
                messages = [json.loads(line) for line in completed.stdout.splitlines()]
                self.assertEqual(completed.returncode, 1)
                self.assertEqual(messages[-1]["code"], code)
                self.assertTrue(messages[-1]["fatal"])

        worker = subprocess.Popen(
            command,
            cwd=ROOT,
            env=environment,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        assert worker.stdin is not None and worker.stdout is not None
        self.assertEqual(json.loads(worker.stdout.readline())["type"], "ready")
        worker.stdin.write("x" * (64 * 1024 + 1))
        worker.stdin.flush()
        selector = selectors.DefaultSelector()
        selector.register(worker.stdout, selectors.EVENT_READ)
        self.assertTrue(selector.select(timeout=2), "oversized input did not fail promptly")
        selector.close()
        error = json.loads(worker.stdout.readline())
        self.assertEqual(error["code"], "message_too_large")
        self.assertEqual(worker.wait(timeout=5), 1)
        worker.stdin.close()
        worker.stdout.close()
        assert worker.stderr is not None
        self.assertEqual(worker.stderr.read(), "")
        worker.stderr.close()

    def test_worker_truncates_result_tail_to_its_output_bound(self) -> None:
        first = {"text": "x" * 100}
        message = {"results": [first, {"text": "y" * 100}]}
        maximum = len(_encode({"results": [first], "truncated": False}))
        _fit_results(message, maximum)
        self.assertEqual(message["results"], [first])
        self.assertTrue(message["truncated"])
        self.assertLessEqual(len(_encode(message)), maximum)

        with self.assertRaisesRegex(RuntimeError, "one protocol result"):
            _fit_results({"results": [first]}, 32)


if __name__ == "__main__":
    unittest.main()
