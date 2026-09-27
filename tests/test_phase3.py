"""Offline Phase 3 check: real TUI selection, failure restoration, feedback, and profile."""

from __future__ import annotations

import fcntl
import json
import math
import os
import pty
import select
import struct
import subprocess
import sys
import tempfile
import termios
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "tests/fixtures/corpus/manifest.jsonl"
BENCHMARK = ROOT / "benchmarks/provisional-v1.json"
sys.path.insert(0, str(ROOT / "python"))

from mambomeme_search.build_index import build_snapshot  # noqa: E402
from mambomeme_search.retrieve import SearchEngine  # noqa: E402


def wait_for(master: int, process: subprocess.Popen[bytes], needle: bytes) -> bytes:
    output = bytearray()
    deadline = time.monotonic() + 8
    while needle not in output and time.monotonic() < deadline:
        readable, _, _ = select.select([master], [], [], 0.1)
        if readable:
            try:
                output.extend(os.read(master, 65536))
            except OSError:
                break
        if process.poll() is not None and not readable:
            break
    if needle not in output:
        process.kill()
        process.wait(timeout=2)
        raise AssertionError((needle, bytes(output[-1000:])))
    return bytes(output)


def finish_pty(
    master: int,
    slave: int,
    process: subprocess.Popen[bytes],
    before: list[object],
    output: bytes,
    *,
    expect_screen: bool = True,
    timeout: float = 8,
) -> bytes:
    deadline = time.monotonic() + timeout
    chunks = [output]
    while process.poll() is None and time.monotonic() < deadline:
        readable, _, _ = select.select([master], [], [], 0.1)
        if readable:
            try:
                chunks.append(os.read(master, 65536))
            except OSError:
                break
    assert process.wait(timeout=2) == 0
    while True:
        readable, _, _ = select.select([master], [], [], 0)
        if not readable:
            break
        try:
            chunks.append(os.read(master, 65536))
        except OSError:
            break
    after = termios.tcgetattr(slave)
    os.close(master)
    os.close(slave)
    combined = b"".join(chunks)
    assert after == before
    if expect_screen:
        assert b"\x1b[?1049h" in combined and b"\x1b[?1049l" in combined
        assert b"\x1b[?25h" in combined
    return combined


def start_pty(*arguments: str) -> tuple[int, int, subprocess.Popen[bytes], list[object]]:
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    before = termios.tcgetattr(slave)
    environment = os.environ.copy()
    environment["TERM"] = "xterm-256color"
    environment["PYTHONPATH"] = str(ROOT / "python")
    process = subprocess.Popen(
        [str(ROOT / "target/debug/mambomeme"), *arguments],
        cwd=ROOT,
        env=environment,
        stdin=slave,
        stdout=slave,
        stderr=slave,
        close_fds=True,
    )
    return master, slave, process, before


def main() -> None:
    subprocess.run(
        ["cargo", "build", "--quiet", "--locked"], cwd=ROOT, check=True
    )
    with tempfile.TemporaryDirectory(prefix="mambomeme-phase3-") as temporary:
        data_dir = Path(temporary) / "data"
        subprocess.run(
            [
                str(ROOT / "target/debug/mambomeme"),
                "ingest",
                "--manifest",
                str(MANIFEST),
                "--data-dir",
                str(data_dir),
            ],
            cwd=ROOT,
            check=True,
            capture_output=True,
        )
        build_snapshot(data_dir)
        with SearchEngine(data_dir) as engine:
            expected_id = engine.search("john cena")["results"][0]["id"]

        master, slave, process, before = start_pty(
            "tui", "--data-dir", str(data_dir), "--feedback"
        )
        output = wait_for(master, process, b"Ready")
        os.write(master, b"john cena\r")
        output += wait_for(master, process, b"John Cena")
        os.write(master, b"s")
        output = finish_pty(master, slave, process, before, output)
        assert json.dumps({"selected_id": expected_id}, separators=(",", ":")).encode() in output

        inspected = subprocess.run(
            [
                str(ROOT / "target/debug/mambomeme"),
                "feedback",
                "inspect",
                "--data-dir",
                str(data_dir),
            ],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        )
        events = json.loads(inspected.stdout)
        assert len(events) == 1 and events[0]["action"] == "choose"
        assert events[0]["target_id"] == expected_id and events[0]["shown_rank"] == 1
        assert set(events[0]) == {
            "schema_version",
            "event_id",
            "session_id",
            "timestamp_unix_ms",
            "request_id",
            "query",
            "returned",
            "action",
            "target_id",
            "shown_rank",
            "dataset_version",
            "retriever_version",
            "interface_version",
            "elapsed_since_render_ms",
        }
        subprocess.run(
            [
                str(ROOT / "target/debug/mambomeme"),
                "feedback",
                "delete",
                "--data-dir",
                str(data_dir),
            ],
            cwd=ROOT,
            check=True,
            capture_output=True,
        )
        assert not (data_dir / "feedback/interaction-events.jsonl").exists()

        feedback_path = data_dir / "feedback/interaction-events.jsonl"
        feedback_path.parent.mkdir(parents=True, exist_ok=True)
        feedback_path.write_text("{broken json}\n", encoding="utf-8")
        master, slave, process, before = start_pty("tui", "--data-dir", str(data_dir))
        output = wait_for(master, process, b"disabled:")
        os.write(master, b"\x1b")
        output = finish_pty(master, slave, process, before, output)
        assert b"Ready" in output
        assert feedback_path.read_text(encoding="utf-8") == "{broken json}\n"
        feedback_path.unlink()

        master, slave, process, before = start_pty(
            "tui", "--data-dir", str(data_dir), "--feedback"
        )
        output = wait_for(master, process, b"Ready")
        os.write(master, b"john cena\r")
        output += wait_for(master, process, b"John Cena")
        os.write(master, b"\x1bOQ")  # F2 disables feedback after this result rendered.
        output += wait_for(master, process, b"Feedback disabled")
        os.write(master, b"s")
        finish_pty(master, slave, process, before, output)
        assert not (data_dir / "feedback/interaction-events.jsonl").exists()

        fake_python = Path(temporary) / "fake-python"
        fake_python.write_text(
            f"#!{sys.executable}\n"
            "import json, sys\n"
            "print(json.dumps({'type':'ready','protocol_version':1,'dataset_version':'d',"
            "'default_route':'lexical','retriever_version':'r','routes':['lexical']}), flush=True)\n"
            "sys.stdin.readline()\n",
            encoding="utf-8",
        )
        fake_python.chmod(0o700)
        master, slave, process, before = start_pty(
            "tui",
            "--data-dir",
            str(data_dir),
            "--python",
            str(fake_python),
        )
        output = wait_for(master, process, b"Ready")
        os.write(master, b"john cena\r")
        output += wait_for(master, process, b"Fatal error")
        os.write(master, b"q")
        finish_pty(master, slave, process, before, output)

        report = Path(temporary) / "interface-profile.json"
        master, slave, process, before = start_pty(
            "profile",
            "--data-dir",
            str(data_dir),
            "--benchmark",
            str(BENCHMARK),
            "--output",
            str(report),
        )
        finish_pty(
            master,
            slave,
            process,
            before,
            b"",
            expect_screen=False,
            timeout=30,
        )
        profile = json.loads(report.read_text(encoding="utf-8"))
        assert profile["warmups"] == 128 and profile["measurements"] == 1_024
        assert profile["backend"] == "crossterm-pty"
        assert profile["viewport"] == "80x24" and profile["result_limit"] == 10
        assert {
            "query_projection",
            "operating_system",
            "architecture",
            "build_profile",
            "package_version",
            "protocol_version",
            "rust_minimum_version",
            "python_version",
            "terminal",
            "cpu_model",
            "cargo_lock_sha256",
        } <= profile.keys()
        assert all(
            math.isfinite(profile[key]) and profile[key] >= 0
            for key in (
                "worker_startup_ms",
                "submit_to_render_p50_ms",
                "submit_to_render_p95_ms",
                "submit_to_render_p99_ms",
                "request_error_rate",
            )
        )
        assert profile["request_error_rate"] <= 1
        assert profile["latency_gate_pass"] and profile["reliability_gate_pass"]
        assert profile["mmts_search_v1"] == {
            "status": "INCOMPLETE",
            "score": None,
            "missing_inputs": [
                "independent human-labelled hidden benchmark with safety subset"
            ],
        }

        active_dataset = json.loads(
            (data_dir / "active.json").read_text(encoding="utf-8")
        )["dataset_version"]
        error_python = Path(temporary) / "error-python"
        error_python.write_text(
            f"#!{sys.executable}\n"
            "import json, sys\n"
            "if '--version' in sys.argv:\n"
            "    print('Python fixture')\n"
            "    raise SystemExit\n"
            f"print(json.dumps({{'type':'ready','protocol_version':1,'dataset_version':{active_dataset!r},"
            "'default_route':'lexical','retriever_version':'fixture','routes':['lexical']}), flush=True)\n"
            "for line in sys.stdin:\n"
            "    message = json.loads(line)\n"
            "    if message['type'] == 'shutdown':\n"
            "        print(json.dumps({'type':'bye','protocol_version':1,'request_id':message['request_id']}), flush=True)\n"
            "        break\n"
            "    print(json.dumps({'type':'error','protocol_version':1,'request_id':message['request_id'],"
            "'code':'fixture_error','message':'expected failure','fatal':False}), flush=True)\n",
            encoding="utf-8",
        )
        error_python.chmod(0o700)
        error_report = Path(temporary) / "interface-error-profile.json"
        master, slave, process, before = start_pty(
            "profile",
            "--data-dir",
            str(data_dir),
            "--benchmark",
            str(BENCHMARK),
            "--output",
            str(error_report),
            "--python",
            str(error_python),
        )
        finish_pty(
            master,
            slave,
            process,
            before,
            b"",
            expect_screen=False,
            timeout=30,
        )
        failed_profile = json.loads(error_report.read_text(encoding="utf-8"))
        assert failed_profile["request_error_rate"] == 1
        assert not failed_profile["reliability_gate_pass"]
        assert failed_profile["mmts_search_v1"]["status"] == "INCOMPLETE"

    print("Phase 3 fixture passed")


if __name__ == "__main__":
    main()
