"""Build and atomically publish a deterministic lexical-search snapshot."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import sqlite3
import tempfile
from contextlib import closing
from pathlib import Path
from typing import Any, Iterable

from .dense import DenseIndex, build_lsa_artifacts
from .text import normalize_search_text


BUILDER_VERSION = "phase2-fts-lsa-v1"
NORMALIZATION_VERSION = "nfkc-whitespace-v1"
SEARCH_DOCUMENT_VERSION = "fielded-fts-lsa-v1"
REQUIRED_SCHEMA_VERSION = 1
MAX_MEDIA_BYTES = 16 * 1024 * 1024
ELIGIBLE_OUTCOMES = ("accepted", "duplicate", "unchanged")
MEME_COLUMNS = (
    "id",
    "kind",
    "title",
    "body_text",
    "asset_uri",
    "language",
    "content_hash",
    "available",
    "safe",
    "reviewed",
    "people",
    "template",
    "tags",
    "ocr",
    "caption",
    "description",
    "processing_version",
)
SEARCH_COLUMNS = (
    "title",
    "people",
    "template",
    "tags",
    "ocr",
    "caption",
    "description",
    "body_text",
)


class BuildError(RuntimeError):
    """A candidate snapshot failed validation and was not published."""


def _json_bytes(value: Any) -> bytes:
    return (
        json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
        + "\n"
    ).encode("utf-8")


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _content_sha256(domain: bytes, content: bytes) -> str:
    digest = hashlib.sha256(domain)
    digest.update(content)
    return digest.hexdigest()


def _write_bytes(path: Path, content: bytes) -> None:
    with path.open("wb") as stream:
        stream.write(content)
        stream.flush()
        os.fsync(stream.fileno())


def _fsync_directory(path: Path) -> None:
    descriptor = os.open(path, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def _connect(path: Path, *, read_only: bool = False) -> sqlite3.Connection:
    if read_only:
        connection = sqlite3.connect(f"file:{path.as_posix()}?mode=ro", uri=True)
    else:
        connection = sqlite3.connect(path)
    connection.row_factory = sqlite3.Row
    connection.execute("PRAGMA foreign_keys = ON")
    return connection


def _copy_database(source: Path, destination: Path) -> None:
    with closing(_connect(source, read_only=True)) as source_db, closing(
        _connect(destination)
    ) as target_db:
        source_db.backup(target_db)


def _require_schema(connection: sqlite3.Connection) -> None:
    table_names = {
        row[0]
        for row in connection.execute(
            "SELECT name FROM sqlite_schema WHERE type = 'table'"
        )
    }
    missing = {
        "schema_version",
        "corpus_state",
        "meme_item",
        "source_item",
        "processing_run",
    } - table_names
    if missing:
        raise BuildError(f"missing required tables: {', '.join(sorted(missing))}")

    versions = [row[0] for row in connection.execute("SELECT version FROM schema_version")]
    if versions != [REQUIRED_SCHEMA_VERSION]:
        raise BuildError(
            f"schema version must be {REQUIRED_SCHEMA_VERSION}, got {versions!r}"
        )

    columns = {
        row[1] for row in connection.execute("PRAGMA table_info(meme_item)")
    }
    missing_columns = set(MEME_COLUMNS) - columns
    if missing_columns:
        raise BuildError(
            f"meme_item is missing columns: {', '.join(sorted(missing_columns))}"
        )
    source_columns = {
        row[1] for row in connection.execute("PRAGMA table_info(source_item)")
    }
    required_source_columns = {
        "meme_item_id",
        "record_hash",
        "canonical_json",
        "safe",
        "reviewed",
        "outcome",
    }
    if missing_source_columns := required_source_columns - source_columns:
        raise BuildError(
            "source_item is missing columns: "
            + ", ".join(sorted(missing_source_columns))
        )


def _check_database(connection: sqlite3.Connection) -> None:
    integrity = [row[0] for row in connection.execute("PRAGMA integrity_check")]
    if integrity != ["ok"]:
        raise BuildError(f"SQLite integrity check failed: {integrity!r}")
    foreign_keys = list(connection.execute("PRAGMA foreign_key_check"))
    if foreign_keys:
        raise BuildError(f"SQLite foreign-key check failed: {foreign_keys!r}")


def _require_completed_ingest(connection: sqlite3.Connection) -> None:
    state = connection.execute(
        "SELECT unresolved_manifest FROM corpus_state WHERE singleton = 1"
    ).fetchone()
    if state is None:
        raise BuildError("corpus state is missing")
    if state["unresolved_manifest"] is not None:
        raise BuildError("a failed or interrupted manifest remains unresolved")

    row = connection.execute(
        """
        SELECT completed_at, fatal_batch, error_summary
        FROM processing_run
        WHERE stage = 'ingest'
        ORDER BY id DESC
        LIMIT 1
        """
    ).fetchone()
    if row is None:
        raise BuildError("no completed ingest run")
    if row["completed_at"] is None or row["fatal_batch"] or row["error_summary"]:
        raise BuildError("latest ingest run is incomplete or failed")


def _eligibility_sql(alias: str = "m") -> str:
    placeholders = ", ".join("?" for _ in ELIGIBLE_OUTCOMES)
    return f"""
        {alias}.available = 1
        AND {alias}.safe = 1
        AND {alias}.reviewed = 1
        AND EXISTS (
            SELECT 1
            FROM source_item AS s
            WHERE s.meme_item_id = {alias}.id
              AND s.deleted = 0
              AND s.outcome IN ({placeholders})
              AND s.safe = 1
              AND s.reviewed = 1
              AND trim(coalesce(s.creator, '')) <> ''
              AND trim(coalesce(s.licence, '')) <> ''
              AND lower(trim(s.licence)) <> 'unknown'
              AND trim(coalesce(s.permission, '')) <> ''
              AND trim(coalesce(s.attribution, '')) <> ''
              AND trim(coalesce(s.retention_policy, '')) <> ''
              AND trim(coalesce(s.redistribution_policy, '')) <> ''
              AND lower(trim(s.redistribution_policy)) = 'redistributable'
        )
    """


def _eligible_rows(connection: sqlite3.Connection) -> list[sqlite3.Row]:
    columns = ", ".join(f"m.{column}" for column in MEME_COLUMNS)
    return list(
        connection.execute(
            f"SELECT {columns} FROM meme_item AS m "
            f"WHERE {_eligibility_sql()} ORDER BY m.id",
            ELIGIBLE_OUTCOMES,
        )
    )


def _decode_string_array(value: str, field: str, item_id: str) -> list[str]:
    try:
        decoded = json.loads(value)
    except (TypeError, json.JSONDecodeError) as error:
        raise BuildError(f"{item_id}: {field} is not valid JSON") from error
    if not isinstance(decoded, list) or any(not isinstance(item, str) for item in decoded):
        raise BuildError(f"{item_id}: {field} must be a JSON array of strings")
    return decoded


def _search_values(row: sqlite3.Row) -> dict[str, str]:
    values = {column: row[column] or "" for column in SEARCH_COLUMNS}
    values["people"] = " ".join(
        _decode_string_array(row["people"], "people", row["id"])
    )
    values["tags"] = " ".join(
        _decode_string_array(row["tags"], "tags", row["id"])
    )
    return values


def _dense_document(row: sqlite3.Row) -> str:
    values = _search_values(row)
    weights = {
        "title": 3,
        "people": 3,
        "template": 3,
        "tags": 2,
        "ocr": 2,
        "caption": 1,
        "description": 1,
        "body_text": 1,
    }
    return " ".join(
        values[field] for field in SEARCH_COLUMNS for _ in range(weights[field])
    )


def _canonical_record(
    connection: sqlite3.Connection, row: sqlite3.Row
) -> dict[str, Any]:
    record = {column: row[column] for column in MEME_COLUMNS}
    record["people"] = _decode_string_array(row["people"], "people", row["id"])
    record["tags"] = _decode_string_array(row["tags"], "tags", row["id"])
    placeholders = ", ".join("?" for _ in ELIGIBLE_OUTCOMES)
    provenance_columns = (
        "source",
        "source_item_id",
        "source_url",
        "media_url",
        "creator",
        "licence",
        "permission",
        "attribution",
        "retention_policy",
        "redistribution_policy",
        "content_hash",
    )
    sources = connection.execute(
        f"""
        SELECT {', '.join(provenance_columns)}
        FROM source_item
        WHERE meme_item_id = ?
          AND deleted = 0
          AND outcome IN ({placeholders})
          AND safe = 1
          AND reviewed = 1
          AND trim(coalesce(creator, '')) <> ''
          AND trim(coalesce(licence, '')) <> ''
          AND lower(trim(licence)) <> 'unknown'
          AND trim(coalesce(permission, '')) <> ''
          AND trim(coalesce(attribution, '')) <> ''
          AND trim(coalesce(retention_policy, '')) <> ''
          AND lower(trim(coalesce(redistribution_policy, ''))) = 'redistributable'
        ORDER BY source, source_item_id
        """,
        (row["id"], *ELIGIBLE_OUTCOMES),
    )
    record["sources"] = [
        {column: source[column] for column in provenance_columns} for source in sources
    ]
    return record


def _validate_content(root: Path, rows: Iterable[sqlite3.Row]) -> None:
    root = root.resolve()
    for row in rows:
        if row["kind"] == "text":
            if row["asset_uri"] is not None or row["body_text"] is None:
                raise BuildError(f"{row['id']}: invalid text content fields")
            actual = _content_sha256(
                b"mambomeme:text:v1\0", row["body_text"].encode("utf-8")
            )
        else:
            if row["body_text"] is not None or not row["asset_uri"]:
                raise BuildError(f"{row['id']}: invalid image content fields")
            relative = Path(row["asset_uri"])
            if relative.is_absolute():
                raise BuildError(f"{row['id']}: asset path must be relative")
            try:
                asset = (root / relative).resolve(strict=True)
            except OSError as error:
                raise BuildError(f"{row['id']}: asset is unavailable: {error}") from error
            if not asset.is_relative_to(root) or not asset.is_file():
                raise BuildError(f"{row['id']}: asset escapes the data directory")
            actual = _content_file_sha256(b"mambomeme:image:v1\0", asset)
        if actual != row["content_hash"]:
            raise BuildError(f"{row['id']}: content checksum mismatch")


def _content_file_sha256(domain: bytes, path: Path) -> str:
    try:
        if path.stat().st_size > MAX_MEDIA_BYTES:
            raise BuildError(f"image exceeds {MAX_MEDIA_BYTES} bytes")
        digest = hashlib.sha256(domain)
        total = 0
        with path.open("rb") as stream:
            while chunk := stream.read(min(1024 * 1024, MAX_MEDIA_BYTES - total + 1)):
                total += len(chunk)
                if total > MAX_MEDIA_BYTES:
                    raise BuildError(f"image exceeds {MAX_MEDIA_BYTES} bytes")
                digest.update(chunk)
        return digest.hexdigest()
    except OSError as error:
        raise BuildError(f"asset is unavailable: {error}") from error


def _build_fts(connection: sqlite3.Connection, rows: Iterable[sqlite3.Row]) -> None:
    try:
        with connection:
            connection.execute("DROP TABLE IF EXISTS meme_fts")
            connection.execute("DROP TABLE IF EXISTS serving_item")
            connection.execute(
                """
                CREATE TABLE serving_item (
                    item_id TEXT PRIMARY KEY REFERENCES meme_item(id)
                ) WITHOUT ROWID
                """
            )
            connection.execute(
                """
                CREATE VIRTUAL TABLE meme_fts USING fts5(
                    item_id UNINDEXED,
                    title,
                    people,
                    template,
                    tags,
                    ocr,
                    caption,
                    description,
                    body_text,
                    tokenize = 'unicode61 remove_diacritics 2'
                )
                """
            )
            for row in rows:
                values = _search_values(row)
                connection.execute(
                    "INSERT INTO serving_item (item_id) VALUES (?)", (row["id"],)
                )
                connection.execute(
                    f"INSERT INTO meme_fts (item_id, {', '.join(SEARCH_COLUMNS)}) "
                    f"VALUES ({', '.join('?' for _ in range(len(SEARCH_COLUMNS) + 1))})",
                    (
                        row["id"],
                        *(normalize_search_text(values[column]) for column in SEARCH_COLUMNS),
                    ),
                )
            placeholders = ", ".join("?" for _ in ELIGIBLE_OUTCOMES)
            connection.execute(
                f"""
                DELETE FROM source_item
                WHERE meme_item_id IS NULL
                   OR meme_item_id NOT IN (SELECT item_id FROM serving_item)
                   OR deleted <> 0
                   OR outcome NOT IN ({placeholders})
                   OR coalesce(safe, 0) <> 1
                   OR coalesce(reviewed, 0) <> 1
                   OR trim(coalesce(creator, '')) = ''
                   OR trim(coalesce(licence, '')) = ''
                   OR lower(trim(coalesce(licence, ''))) = 'unknown'
                   OR trim(coalesce(permission, '')) = ''
                   OR trim(coalesce(attribution, '')) = ''
                   OR trim(coalesce(retention_policy, '')) = ''
                   OR lower(trim(coalesce(redistribution_policy, ''))) <> 'redistributable'
                """,
                ELIGIBLE_OUTCOMES,
            )
            connection.execute(
                "DELETE FROM meme_item WHERE id NOT IN (SELECT item_id FROM serving_item)"
            )
    except sqlite3.Error as error:
        raise BuildError(f"FTS5 index build failed: {error}") from error


def _validate_coverage(connection: sqlite3.Connection, expected_count: int) -> None:
    serving_count = connection.execute("SELECT count(*) FROM serving_item").fetchone()[0]
    fts_count = connection.execute("SELECT count(*) FROM meme_fts").fetchone()[0]
    if (serving_count, fts_count) != (expected_count, expected_count):
        raise BuildError(
            "eligible/serving/FTS coverage mismatch: "
            f"{expected_count}/{serving_count}/{fts_count}"
        )

    eligible = _eligibility_sql("m")
    bad_serving = connection.execute(
        f"""
        SELECT si.item_id
        FROM serving_item AS si
        JOIN meme_item AS m ON m.id = si.item_id
        WHERE NOT ({eligible})
        LIMIT 1
        """,
        ELIGIBLE_OUTCOMES,
    ).fetchone()
    if bad_serving:
        raise BuildError(f"ineligible serving item: {bad_serving[0]}")

    mismatch = connection.execute(
        """
        SELECT item_id FROM serving_item
        EXCEPT SELECT item_id FROM meme_fts
        UNION ALL
        SELECT item_id FROM meme_fts
        EXCEPT SELECT item_id FROM serving_item
        LIMIT 1
        """
    ).fetchone()
    if mismatch:
        raise BuildError(f"serving/FTS item mismatch: {mismatch[0]}")

    try:
        connection.execute("INSERT INTO meme_fts(meme_fts) VALUES ('integrity-check')")
    except sqlite3.Error as error:
        raise BuildError(f"FTS5 integrity check failed: {error}") from error


def _artifact(path: Path) -> dict[str, Any]:
    return {"bytes": path.stat().st_size, "sha256": _sha256(path)}


def _verify_artifacts(snapshot: Path, manifest: dict[str, Any]) -> None:
    for name, expected in manifest["artifacts"].items():
        path = snapshot / name
        if not path.is_file() or _artifact(path) != expected:
            raise BuildError(f"artifact checksum mismatch: {name}")


def _publish_pointer(data_dir: Path, pointer: dict[str, Any]) -> None:
    temporary = data_dir / ".active.json.tmp"
    try:
        _write_bytes(temporary, _json_bytes(pointer))
        os.replace(temporary, data_dir / "active.json")
        try:
            _fsync_directory(data_dir)
        except OSError as error:
            raise BuildError(
                "active pointer was replaced but directory durability is unknown"
            ) from error
    finally:
        temporary.unlink(missing_ok=True)


def build_snapshot(data_dir: str | Path) -> dict[str, Any]:
    """Build a candidate from ``staging/corpus.sqlite`` and publish it."""

    root = Path(data_dir).resolve()
    source = root / "staging" / "corpus.sqlite"
    if not source.is_file():
        raise BuildError(f"staging database not found: {source}")

    snapshots = root / "snapshots"
    snapshots.mkdir(parents=True, exist_ok=True)
    candidate = Path(tempfile.mkdtemp(prefix=".candidate-", dir=snapshots))
    database = candidate / "corpus.sqlite"
    canonical = candidate / "canonical.jsonl"

    try:
        _copy_database(source, database)
        with closing(_connect(database)) as connection:
            compile_options = sorted(
                row[0] for row in connection.execute("PRAGMA compile_options")
            )
            _require_schema(connection)
            _check_database(connection)
            _require_completed_ingest(connection)
            rows = _eligible_rows(connection)
            _validate_content(root, rows)
            canonical_records = [_canonical_record(connection, row) for row in rows]
            dense_documents = [
                (row["id"], _dense_document(row)) for row in rows
            ]
            _build_fts(connection, rows)
            _validate_coverage(connection, len(rows))
            _check_database(connection)
            connection.commit()
            connection.execute("PRAGMA journal_mode = DELETE")
            connection.execute("VACUUM")

        _write_bytes(canonical, b"".join(_json_bytes(row) for row in canonical_records))
        dense_metadata = build_lsa_artifacts(dense_documents, candidate)
        dense_index = DenseIndex(candidate)
        if dense_index.ids != [row["id"] for row in rows]:
            raise BuildError("dense IDs do not match eligible items")
        if dense_metadata["dense_dimension"] != dense_index.components.shape[0]:
            raise BuildError("dense dimension does not match built artifacts")
        if dense_metadata["dense_vocabulary_size"] != len(dense_index.vocabulary):
            raise BuildError("dense vocabulary size does not match built artifacts")
        canonical_identity = _sha256(canonical)
        artifacts = {
            "canonical.jsonl": _artifact(canonical),
            "corpus.sqlite": _artifact(database),
            **{
                name: _artifact(candidate / name)
                for name in (
                    "dense_components.npy",
                    "dense_idf.npy",
                    "dense_ids.json",
                    "dense_vectors.npy",
                    "dense_vocab.json",
                )
            },
        }
        manifest = {
            "artifacts": artifacts,
            "builder_version": BUILDER_VERSION,
            "canonical_identity": canonical_identity,
            "dataset_version": canonical_identity,
            "item_count": len(canonical_records),
            "normalization_version": NORMALIZATION_VERSION,
            "representation": "fielded_fts5+tfidf_lsa",
            "schema_version": REQUIRED_SCHEMA_VERSION,
            "search_document_version": SEARCH_DOCUMENT_VERSION,
            "sqlite_compile_options": compile_options,
            "sqlite_version": sqlite3.sqlite_version,
            **dense_metadata,
        }
        snapshot_id = hashlib.sha256(_json_bytes(manifest)).hexdigest()
        manifest["snapshot_id"] = snapshot_id
        _write_bytes(candidate / "manifest.json", _json_bytes(manifest))
        _verify_artifacts(candidate, manifest)
        _fsync_directory(candidate)

        final_snapshot = snapshots / snapshot_id
        if final_snapshot.exists():
            with (final_snapshot / "manifest.json").open(
                "r", encoding="utf-8"
            ) as stream:
                existing = json.load(stream)
            if existing != manifest:
                raise BuildError(f"snapshot collision: {snapshot_id}")
            _verify_artifacts(final_snapshot, existing)
            shutil.rmtree(candidate)
        else:
            os.replace(candidate, final_snapshot)
            _fsync_directory(snapshots)

        pointer = {
            "dataset_version": canonical_identity,
            "manifest": f"snapshots/{snapshot_id}/manifest.json",
            "snapshot_id": snapshot_id,
        }
        _publish_pointer(root, pointer)
        return manifest
    except Exception:
        if candidate.exists():
            shutil.rmtree(candidate)
        raise


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--data-dir", required=True, type=Path)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        manifest = build_snapshot(args.data_dir)
    except (BuildError, OSError, sqlite3.Error) as error:
        _parser().exit(1, f"mambomeme build-index: {error}\n")
    print(json.dumps(manifest, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
