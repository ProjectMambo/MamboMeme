"""Validated lexical, dense, and hybrid meme retrieval."""

from __future__ import annotations

import hashlib
import json
import sqlite3
from pathlib import Path
from typing import Any

from .build_index import (
    BUILDER_VERSION,
    NORMALIZATION_VERSION,
    REQUIRED_SCHEMA_VERSION,
    SEARCH_DOCUMENT_VERSION,
    _json_bytes,
    _sha256,
)
from .dense import DenseIndex, MODEL_VERSION
from .text import normalize_search_text, search_tokens


PROTOCOL_VERSION = 1
RETRIEVER_VERSION = "bm25-lsa-rrf-v1"
DEFAULT_ROUTE = "lexical"
MAX_CUES = 4
MAX_QUERY_BYTES = 512
MAX_RESULTS = 50
CANDIDATE_DEPTH = 100
RRF_K = 60
DENSE_MINIMUM_SIMILARITY = 0.15
TEMPLATE_LIMIT = 2
FTS_WEIGHTS = (0.0, 12.0, 12.0, 10.0, 5.0, 8.0, 3.0, 3.0, 3.0)
SEARCH_FIELDS = (
    "title",
    "people",
    "template",
    "tags",
    "ocr",
    "caption",
    "description",
    "body_text",
)
REQUIRED_ARTIFACTS = {
    "canonical.jsonl",
    "corpus.sqlite",
    "dense_components.npy",
    "dense_idf.npy",
    "dense_ids.json",
    "dense_vectors.npy",
    "dense_vocab.json",
}


class QueryError(ValueError):
    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code


class SearchEngine:
    """Read-only search engine for one published snapshot."""

    def __init__(self, data_dir: str | Path) -> None:
        self.root = Path(data_dir).resolve()
        pointer = _read_json(self.root / "active.json", 64 * 1024)
        relative_manifest = Path(_required_string(pointer, "manifest"))
        if relative_manifest.is_absolute():
            raise ValueError("active manifest path must be relative")
        manifest_path = (self.root / relative_manifest).resolve(strict=True)
        if not manifest_path.is_relative_to(self.root / "snapshots"):
            raise ValueError("active manifest escapes the snapshot directory")
        self.snapshot = manifest_path.parent
        self.manifest = _read_json(manifest_path, 1024 * 1024)
        snapshot_id = _required_string(self.manifest, "snapshot_id")
        unsigned = {key: value for key, value in self.manifest.items() if key != "snapshot_id"}
        if hashlib.sha256(_json_bytes(unsigned)).hexdigest() != snapshot_id:
            raise ValueError("snapshot manifest identity mismatch")
        if self.snapshot.name != snapshot_id:
            raise ValueError("snapshot directory identity mismatch")
        if pointer.get("snapshot_id") != snapshot_id:
            raise ValueError("active pointer snapshot mismatch")
        if pointer.get("dataset_version") != self.manifest.get("dataset_version"):
            raise ValueError("active pointer dataset mismatch")
        artifacts = self.manifest.get("artifacts")
        if not isinstance(artifacts, dict) or set(artifacts) != REQUIRED_ARTIFACTS:
            raise ValueError("snapshot artifacts do not match the required set")
        expected_versions = {
            "builder_version": BUILDER_VERSION,
            "normalization_version": NORMALIZATION_VERSION,
            "representation": "fielded_fts5+tfidf_lsa",
            "schema_version": REQUIRED_SCHEMA_VERSION,
            "search_document_version": SEARCH_DOCUMENT_VERSION,
        }
        for field, expected in expected_versions.items():
            if self.manifest.get(field) != expected:
                raise ValueError(f"unsupported snapshot {field}")
        for name, expected in artifacts.items():
            path = (self.snapshot / name).resolve(strict=True)
            if not path.is_relative_to(self.snapshot) or not path.is_file():
                raise ValueError(f"artifact path is invalid: {name}")
            if not isinstance(expected, dict) or expected != {
                "bytes": path.stat().st_size,
                "sha256": _sha256(path),
            }:
                raise ValueError(f"artifact checksum mismatch: {name}")

        database = self.snapshot / "corpus.sqlite"
        self.connection = sqlite3.connect(
            f"{database.as_uri()}?mode=ro&immutable=1", uri=True
        )
        self.connection.row_factory = sqlite3.Row
        self.connection.execute("PRAGMA query_only = ON")
        self.items = self._load_items()
        self.dense = DenseIndex(self.snapshot)
        if set(self.dense.ids) != set(self.items):
            raise ValueError("dense IDs do not match serving items")
        if self.manifest.get("item_count") != len(self.items):
            raise ValueError("snapshot item count mismatch")
        if self.manifest.get("dense_model") != MODEL_VERSION:
            raise ValueError("dense model version mismatch")
        if self.manifest.get("dense_dimension") != self.dense.components.shape[0]:
            raise ValueError("dense dimension mismatch")
        if self.manifest.get("dense_vocabulary_size") != len(self.dense.vocabulary):
            raise ValueError("dense vocabulary size mismatch")
        self.dataset_version = _required_string(self.manifest, "dataset_version")

    def close(self) -> None:
        self.connection.close()

    def __enter__(self) -> SearchEngine:
        return self

    def __exit__(self, *_: object) -> None:
        self.close()

    def _load_items(self) -> dict[str, dict[str, Any]]:
        items: dict[str, dict[str, Any]] = {}
        for row in self.connection.execute(
            """
            SELECT m.*
            FROM serving_item AS serving
            JOIN meme_item AS m ON m.id = serving.item_id
            ORDER BY m.id
            """
        ):
            if not row["available"] or not row["safe"] or not row["reviewed"]:
                raise ValueError(f"ineligible serving item: {row['id']}")
            item = dict(row)
            item["people"] = _string_list(row["people"], "people", row["id"])
            item["tags"] = _string_list(row["tags"], "tags", row["id"])
            items[row["id"]] = item
        for source in self.connection.execute(
            """
            SELECT meme_item_id, source, source_url, attribution
            FROM source_item
            WHERE meme_item_id IS NOT NULL
            ORDER BY meme_item_id, source, source_item_id
            """
        ):
            item = items.get(source["meme_item_id"])
            if item is not None and "source" not in item:
                item.update(
                    source=source["source"],
                    source_url=source["source_url"],
                    attribution=source["attribution"],
                )
        if any("source" not in item for item in items.values()):
            raise ValueError("serving item has no provenance")
        return items

    def search(
        self,
        cues: str | list[str],
        *,
        limit: int = 10,
        kind: str | None = None,
        language: str | None = None,
        route: str = DEFAULT_ROUTE,
    ) -> dict[str, Any]:
        normalized = _validate_query(cues, limit, kind, language, route)
        cue_list, limit, kind, language, route = normalized
        evidence: dict[str, dict[str, Any]] = {}
        degraded: set[str] = set()

        for cue in cue_list:
            if route in {"lexical", "hybrid"}:
                self._add_route(
                    evidence,
                    "lexical",
                    self._lexical(cue, kind, language),
                )
            if route in {"dense", "hybrid"}:
                try:
                    dense = self._dense(cue, kind, language)
                except Exception:
                    if route == "dense":
                        raise
                    degraded.add("dense")
                else:
                    self._add_route(evidence, "dense", dense)

        ordered = sorted(evidence, key=lambda item_id: (-evidence[item_id]["fused"], item_id))
        results = []
        template_counts: dict[str, int] = {}
        cue_tokens = [token for cue in cue_list for token in search_tokens(cue)]
        for item_id in ordered:
            item = self.items[item_id]
            template = normalize_search_text(item.get("template") or "").casefold()
            group = template or item_id
            if template_counts.get(group, 0) >= TEMPLATE_LIMIT:
                continue
            template_counts[group] = template_counts.get(group, 0) + 1
            route_evidence = evidence[item_id]
            results.append(
                _result(
                    item,
                    len(results) + 1,
                    cue_tokens,
                    route_evidence,
                    self.dataset_version,
                )
            )
            if len(results) == limit:
                break

        return {
            "dataset_version": self.dataset_version,
            "degraded_routes": sorted(degraded),
            "results": results,
            "retriever_version": RETRIEVER_VERSION,
            "route": route,
        }

    @staticmethod
    def _add_route(
        evidence: dict[str, dict[str, Any]],
        route: str,
        candidates: list[tuple[str, float]],
    ) -> None:
        for rank, (item_id, raw_score) in enumerate(candidates, 1):
            item = evidence.setdefault(
                item_id,
                {"fused": 0.0, "routes": set(), "ranks": {}, "raw": {}},
            )
            item["fused"] += 1.0 / (RRF_K + rank)
            item["routes"].add(route)
            item["ranks"][route] = min(rank, item["ranks"].get(route, rank))
            item["raw"][route] = max(raw_score, item["raw"].get(route, raw_score))

    def _lexical(
        self, cue: str, kind: str | None, language: str | None
    ) -> list[tuple[str, float]]:
        expression = _fts_expression(cue)
        if expression is None:
            return []
        filters = []
        parameters: list[Any] = [expression]
        if kind is not None:
            filters.append("m.kind = ?")
            parameters.append(kind)
        if language is not None:
            filters.append("lower(m.language) = ?")
            parameters.append(language)
        where = " AND " + " AND ".join(filters) if filters else ""
        parameters.append(CANDIDATE_DEPTH)
        weights = ", ".join(str(weight) for weight in FTS_WEIGHTS)
        rows = self.connection.execute(
            f"""
            SELECT f.item_id, bm25(meme_fts, {weights}) AS score
            FROM meme_fts AS f
            JOIN meme_item AS m ON m.id = f.item_id
            WHERE meme_fts MATCH ?{where}
            ORDER BY score, f.item_id
            LIMIT ?
            """,
            parameters,
        )
        return [(row["item_id"], -float(row["score"])) for row in rows]

    def _dense(
        self, cue: str, kind: str | None, language: str | None
    ) -> list[tuple[str, float]]:
        candidates = []
        for item_id, score in self.dense.search(cue, DENSE_MINIMUM_SIMILARITY):
            item = self.items[item_id]
            if kind is not None and item["kind"] != kind:
                continue
            if language is not None and item["language"].casefold() != language:
                continue
            candidates.append((item_id, score))
            if len(candidates) == CANDIDATE_DEPTH:
                break
        return candidates


def _read_json(path: Path, maximum: int) -> dict[str, Any]:
    with path.open("rb") as stream:
        content = stream.read(maximum + 1)
    if len(content) > maximum:
        raise ValueError(f"JSON artifact exceeds {maximum} bytes: {path.name}")
    value = json.loads(content)
    if not isinstance(value, dict):
        raise ValueError(f"JSON artifact must be an object: {path.name}")
    return value


def _required_string(value: dict[str, Any], field: str) -> str:
    result = value.get(field)
    if not isinstance(result, str) or not result:
        raise ValueError(f"missing {field}")
    return result


def _string_list(value: str, field: str, item_id: str) -> list[str]:
    decoded = json.loads(value)
    if not isinstance(decoded, list) or any(not isinstance(item, str) for item in decoded):
        raise ValueError(f"{item_id}: {field} must be an array of strings")
    return decoded


def _validate_query(
    cues: str | list[str],
    limit: int,
    kind: str | None,
    language: str | None,
    route: str,
) -> tuple[list[str], int, str | None, str | None, str]:
    if isinstance(cues, str):
        cues = [cues]
    if not isinstance(cues, list) or not 1 <= len(cues) <= MAX_CUES:
        raise QueryError("invalid_cues", f"cues must contain 1..{MAX_CUES} strings")
    normalized = []
    for cue in cues:
        if not isinstance(cue, str):
            raise QueryError("invalid_cues", "every cue must be a string")
        cue = normalize_search_text(cue)
        if not cue or len(cue.encode("utf-8")) > MAX_QUERY_BYTES:
            raise QueryError(
                "invalid_cue", f"each cue must contain 1..{MAX_QUERY_BYTES} UTF-8 bytes"
            )
        if cue not in normalized:
            normalized.append(cue)
    if isinstance(limit, bool) or not isinstance(limit, int) or not 1 <= limit <= MAX_RESULTS:
        raise QueryError("invalid_limit", f"limit must be an integer in 1..{MAX_RESULTS}")
    if kind is not None and (
        not isinstance(kind, str) or kind not in ("text", "image")
    ):
        raise QueryError("invalid_kind", "kind must be text, image, or null")
    if language is not None:
        if (
            not isinstance(language, str)
            or not 1 <= len(language) <= 35
            or any(not (character.isalnum() or character == "-") for character in language)
        ):
            raise QueryError("invalid_language", "language must be a simple language tag")
        language = language.casefold()
    if not isinstance(route, str) or route not in ("lexical", "dense", "hybrid"):
        raise QueryError("invalid_route", "route must be lexical, dense, or hybrid")
    return normalized, limit, kind, language, route


def _fts_expression(cue: str) -> str | None:
    tokens = search_tokens(cue)
    if not tokens:
        return None
    return " OR ".join(f'"{token}"*' if len(token) >= 4 else f'"{token}"' for token in tokens)


def _result(
    item: dict[str, Any],
    rank: int,
    cue_tokens: list[str],
    evidence: dict[str, Any],
    dataset_version: str,
) -> dict[str, Any]:
    fields = {
        "title": item.get("title") or "",
        "people": " ".join(item.get("people") or []),
        "template": item.get("template") or "",
        "tags": " ".join(item.get("tags") or []),
        "ocr": item.get("ocr") or "",
        "caption": item.get("caption") or "",
        "description": item.get("description") or "",
        "body_text": item.get("body_text") or "",
    }
    matched_fields = []
    for field, value in fields.items():
        tokens = search_tokens(value)
        if any(candidate.startswith(cue) for cue in cue_tokens for candidate in tokens):
            matched_fields.append(field)
    return {
        "asset_uri": item.get("asset_uri"),
        "attribution": item["attribution"],
        "caption": item.get("caption"),
        "dataset_version": dataset_version,
        "id": item["id"],
        "kind": item["kind"],
        "language": item["language"],
        "matched_fields": matched_fields,
        "people": item["people"],
        "rank": rank,
        "retriever_version": RETRIEVER_VERSION,
        "routes": sorted(evidence["routes"]),
        "safe": bool(item["safe"]),
        "scores": {
            "dense_rank": evidence["ranks"].get("dense"),
            "fused": evidence["fused"],
            "lexical_rank": evidence["ranks"].get("lexical"),
        },
        "source": item["source"],
        "source_url": item["source_url"],
        "tags": item["tags"],
        "template": item.get("template"),
        "text": item.get("body_text") or item.get("ocr"),
        "title": item["title"],
    }
