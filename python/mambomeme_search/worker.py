"""Long-lived newline-delimited JSON search worker."""

from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path
from typing import Any

from .retrieve import (
    DEFAULT_ROUTE,
    PROTOCOL_VERSION,
    RETRIEVER_VERSION,
    QueryError,
    SearchEngine,
)


MAX_INPUT_MESSAGE_BYTES = 64 * 1024
MAX_OUTPUT_MESSAGE_BYTES = 16 * 1024 * 1024


def _encode(message: dict[str, Any]) -> bytes:
    return (
        json.dumps(message, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
        + "\n"
    ).encode("utf-8")


def _emit(message: dict[str, Any]) -> None:
    content = _encode(message)
    if len(content) > MAX_OUTPUT_MESSAGE_BYTES:
        raise RuntimeError("protocol output exceeds 16777216 bytes")
    sys.stdout.buffer.write(content)
    sys.stdout.buffer.flush()


def _fit_results(message: dict[str, Any], maximum: int = MAX_OUTPUT_MESSAGE_BYTES) -> None:
    results = message["results"]
    message["truncated"] = False
    while len(results) > 1 and len(_encode(message)) > maximum:
        results.pop()
        message["truncated"] = True
    if len(_encode(message)) > maximum:
        raise RuntimeError("one protocol result exceeds the output limit")


def _error(
    code: str, message: str, request_id: str | None = None, *, fatal: bool = False
) -> dict[str, Any]:
    return {
        "code": code,
        "fatal": fatal,
        "message": message,
        "protocol_version": PROTOCOL_VERSION,
        "request_id": request_id,
        "type": "error",
    }


def _request_id(message: dict[str, Any]) -> str:
    request_id = message.get("request_id")
    if not isinstance(request_id, str) or not 1 <= len(request_id) <= 128:
        raise QueryError("invalid_request_id", "request_id must contain 1..128 characters")
    return request_id


def _decode_message(line: bytes) -> dict[str, Any]:
    try:
        message = json.loads(line)
    except (json.JSONDecodeError, UnicodeDecodeError, ValueError) as error:
        raise QueryError("invalid_json", str(error)) from error
    if not isinstance(message, dict):
        raise QueryError("invalid_message", "protocol message must be an object")
    return message


def _search_request(message: dict[str, Any]) -> dict[str, Any]:
    allowed = {
        "cues",
        "filters",
        "limit",
        "protocol_version",
        "request_id",
        "route",
        "type",
    }
    if unknown := set(message) - allowed:
        raise QueryError("unknown_field", f"unknown search fields: {', '.join(sorted(unknown))}")
    filters = message.get("filters", {})
    if not isinstance(filters, dict) or set(filters) - {"kind", "language"}:
        raise QueryError("invalid_filters", "filters may contain only kind and language")
    cues = message.get("cues")
    if not isinstance(cues, list):
        raise QueryError("invalid_cues", "cues must be an array")
    return {
        "cues": cues,
        "kind": filters.get("kind"),
        "language": filters.get("language"),
        "limit": message.get("limit", 10),
        "route": message.get("route", DEFAULT_ROUTE),
    }


def run(data_dir: Path) -> int:
    try:
        engine = SearchEngine(data_dir)
    except Exception as error:
        _emit(_error("startup_error", str(error), fatal=True))
        return 1

    with engine:
        _emit(
            {
                "dataset_version": engine.dataset_version,
                "default_route": DEFAULT_ROUTE,
                "protocol_version": PROTOCOL_VERSION,
                "retriever_version": RETRIEVER_VERSION,
                "routes": ["lexical", "dense", "hybrid"],
                "type": "ready",
            }
        )
        seen_request_ids: set[str] = set()
        while True:
            message: dict[str, Any] | None = None
            line = sys.stdin.buffer.readline(MAX_INPUT_MESSAGE_BYTES + 1)
            if not line:
                _emit(_error("premature_eof", "protocol input ended before shutdown", fatal=True))
                return 1
            if len(line) > MAX_INPUT_MESSAGE_BYTES:
                _emit(_error("message_too_large", "protocol line exceeds 65536 bytes", fatal=True))
                return 1
            if not line.endswith(b"\n"):
                _emit(_error("partial_line", "protocol input ended before newline", fatal=True))
                return 1
            try:
                message = _decode_message(line)
                request_id = _request_id(message)
                if request_id in seen_request_ids:
                    raise QueryError("duplicate_request_id", "request_id was already used")
                # ponytail: launch-scoped set; bound it if the worker becomes a long-lived service.
                seen_request_ids.add(request_id)
                protocol_version = message.get("protocol_version")
                if (
                    type(protocol_version) is not int
                    or protocol_version != PROTOCOL_VERSION
                ):
                    _emit(
                        _error(
                            "protocol_mismatch",
                            f"expected protocol_version {PROTOCOL_VERSION}",
                            request_id,
                            fatal=True,
                        )
                    )
                    return 1
                message_type = message.get("type")
                if message_type == "shutdown":
                    if set(message) != {"protocol_version", "request_id", "type"}:
                        raise QueryError("unknown_field", "shutdown has unknown fields")
                    _emit(
                        {
                            "protocol_version": PROTOCOL_VERSION,
                            "request_id": request_id,
                            "type": "bye",
                        }
                    )
                    return 0
                if message_type != "search":
                    raise QueryError("unknown_type", "type must be search or shutdown")
                arguments = _search_request(message)
                started = time.perf_counter_ns()
                response = engine.search(**arguments)
                elapsed_ms = (time.perf_counter_ns() - started) / 1_000_000
                result_message = {
                    **response,
                    "elapsed_ms": elapsed_ms,
                    "protocol_version": PROTOCOL_VERSION,
                    "request_id": request_id,
                    "type": "results",
                }
                _fit_results(result_message)
                _emit(result_message)
            except QueryError as error:
                request_id = message.get("request_id") if isinstance(message, dict) else None
                request_id = request_id if isinstance(request_id, str) else None
                _emit(_error(error.code, str(error), request_id))
            except Exception as error:
                request_id = message.get("request_id") if isinstance(message, dict) else None
                request_id = request_id if isinstance(request_id, str) else None
                _emit(_error("search_error", str(error), request_id, fatal=True))
                return 1


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--data-dir", required=True, type=Path)
    return parser


def main(argv: list[str] | None = None) -> int:
    return run(_parser().parse_args(argv).data_dir)


if __name__ == "__main__":
    raise SystemExit(main())
