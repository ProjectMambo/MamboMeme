from __future__ import annotations

import hashlib
import json
import sqlite3
import sys
import tempfile
import unittest
from contextlib import closing
from pathlib import Path
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "python"))

from mambomeme_search.build_index import (  # noqa: E402
    BuildError,
    build_snapshot,
    normalize_search_text,
)


class BuildIndexTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.data_dir = Path(self.temporary.name)
        staging = self.data_dir / "staging"
        staging.mkdir()
        self.database = staging / "corpus.sqlite"
        with closing(sqlite3.connect(self.database)) as connection:
            connection.executescript((ROOT / "migrations" / "001_initial.sql").read_text())
            connection.execute(
                """
                INSERT INTO meme_item VALUES (
                    'john-cena', 'text', 'John Cena — You Can''t See Me',
                    'You can''t see me.', NULL, 'en', ?, 1, 1, 1,
                    '["John Cena"]', 'you can''t see me',
                    '["wrestling","WWE"]', 'you can''t see me', NULL,
                    'A wrestling reaction about being invisible.', 'fixture-v1'
                )
                """,
                (
                    hashlib.sha256(
                        b"mambomeme:text:v1\0" + b"You can't see me."
                    ).hexdigest(),
                ),
            )
            connection.execute(
                """
                INSERT INTO source_item (
                    source, source_item_id, meme_item_id, kind, source_url,
                    creator, licence, permission, attribution, retention_policy,
                    redistribution_policy, fetched_at, content_hash, safe,
                    reviewed, outcome
                ) VALUES (
                    'fixture', 'john-cena', 'john-cena', 'text',
                    'https://example.invalid/john-cena', 'Fixture author', 'CC0-1.0',
                    'fixture-authored', 'Fixture attribution', 'local-fixture',
                    'redistributable', '2026-09-24T00:00:00Z', ?, 1, 1,
                    'accepted'
                )
                """,
                (
                    hashlib.sha256(
                        b"mambomeme:text:v1\0" + b"You can't see me."
                    ).hexdigest(),
                ),
            )
            connection.execute(
                """
                INSERT INTO processing_run (
                    stage, input_version, output_version, started_at,
                    completed_at, tool_versions
                ) VALUES ('ingest', 'v1', 'v1', '0', '1', '{}')
                """
            )
            connection.commit()

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_rebuild_is_deterministic(self) -> None:
        first = build_snapshot(self.data_dir)
        first_pointer = (self.data_dir / "active.json").read_bytes()
        second = build_snapshot(self.data_dir)

        self.assertEqual(first, second)
        self.assertEqual(first_pointer, (self.data_dir / "active.json").read_bytes())
        self.assertEqual(first["item_count"], 1)

        pointer = json.loads(first_pointer)
        snapshot = self.data_dir / Path(pointer["manifest"]).parent
        with closing(sqlite3.connect(snapshot / "corpus.sqlite")) as connection:
            result = connection.execute(
                "SELECT item_id FROM meme_fts WHERE meme_fts MATCH 'john cena'"
            ).fetchall()
        self.assertEqual(result, [("john-cena",)])

    def test_failed_publication_preserves_active_pointer(self) -> None:
        build_snapshot(self.data_dir)
        active_before = (self.data_dir / "active.json").read_bytes()
        with closing(sqlite3.connect(self.database)) as connection:
            connection.execute(
                "UPDATE meme_item SET title = 'Changed but still eligible'"
            )
            connection.commit()

        with mock.patch(
            "mambomeme_search.build_index._publish_pointer",
            side_effect=OSError("simulated publication failure"),
        ):
            with self.assertRaisesRegex(OSError, "simulated publication failure"):
                build_snapshot(self.data_dir)

        self.assertEqual(active_before, (self.data_dir / "active.json").read_bytes())
        self.assertEqual(list((self.data_dir / "snapshots").glob(".candidate-*")), [])

    def test_invalid_dense_alignment_cannot_publish(self) -> None:
        with mock.patch(
            "mambomeme_search.build_index.DenseIndex",
            return_value=mock.Mock(ids=[]),
        ):
            with self.assertRaisesRegex(BuildError, "dense IDs"):
                build_snapshot(self.data_dir)
        self.assertFalse((self.data_dir / "active.json").exists())
        self.assertEqual(list((self.data_dir / "snapshots").glob(".candidate-*")), [])

    def test_search_normalization_is_nfkc_and_collapses_whitespace(self) -> None:
        self.assertEqual(normalize_search_text("  Ｊohn\tCena\n"), "John Cena")

    def test_link_only_rights_publish_an_empty_replacement(self) -> None:
        first = build_snapshot(self.data_dir)
        with closing(sqlite3.connect(self.database)) as connection:
            connection.execute(
                "UPDATE source_item SET redistribution_policy = 'link-only'"
            )
            connection.commit()

        replacement = build_snapshot(self.data_dir)
        self.assertEqual(replacement["item_count"], 0)
        self.assertNotEqual(first["snapshot_id"], replacement["snapshot_id"])
        snapshot = self.data_dir / "snapshots" / replacement["snapshot_id"]
        with closing(sqlite3.connect(snapshot / "corpus.sqlite")) as connection:
            self.assertEqual(connection.execute("SELECT count(*) FROM meme_item").fetchone()[0], 0)
            self.assertEqual(
                connection.execute("SELECT count(*) FROM source_item").fetchone()[0],
                0,
            )

    def test_missing_media_cannot_replace_the_active_snapshot(self) -> None:
        media = self.data_dir / "media" / "sha256"
        image = b"P3\n1 1\n255\n0 0 0\n"
        digest = hashlib.sha256(b"mambomeme:image:v1\0" + image).hexdigest()
        asset = media / digest[:2] / f"{digest}.ppm"
        asset.parent.mkdir(parents=True)
        asset.write_bytes(image)
        with closing(sqlite3.connect(self.database)) as connection:
            connection.execute(
                """
                INSERT INTO meme_item VALUES (
                    'image-item', 'image', 'Image item', NULL, ?, 'en', ?,
                    1, 1, 1, '[]', NULL, '[]', NULL, 'one black pixel',
                    'fixture image', 'fixture-v1'
                )
                """,
                (asset.relative_to(self.data_dir).as_posix(), digest),
            )
            connection.execute(
                """
                INSERT INTO source_item (
                    source, source_item_id, meme_item_id, kind, source_url,
                    creator, licence, permission, attribution, retention_policy,
                    redistribution_policy, fetched_at, content_hash, safe,
                    reviewed, outcome
                ) VALUES (
                    'fixture', 'image-item', 'image-item', 'image',
                    'https://example.invalid/image', 'Fixture author', 'CC0-1.0',
                    'fixture-authored', 'Fixture attribution', 'local-fixture',
                    'redistributable', '2026-09-24T00:00:00Z', ?, 1, 1,
                    'accepted'
                )
                """,
                (digest,),
            )
            connection.commit()

        build_snapshot(self.data_dir)
        active_before = (self.data_dir / "active.json").read_bytes()
        asset.write_bytes(b"corrupt")
        with self.assertRaisesRegex(BuildError, "content checksum mismatch"):
            build_snapshot(self.data_dir)
        self.assertEqual(active_before, (self.data_dir / "active.json").read_bytes())
        with asset.open("wb") as stream:
            stream.truncate(16 * 1024 * 1024 + 1)
        with self.assertRaisesRegex(BuildError, "image exceeds"):
            build_snapshot(self.data_dir)
        self.assertEqual(active_before, (self.data_dir / "active.json").read_bytes())
        asset.unlink()
        with self.assertRaisesRegex(BuildError, "asset is unavailable"):
            build_snapshot(self.data_dir)
        self.assertEqual(active_before, (self.data_dir / "active.json").read_bytes())

    def test_failed_ingest_cannot_replace_the_active_snapshot(self) -> None:
        build_snapshot(self.data_dir)
        active_before = (self.data_dir / "active.json").read_bytes()
        with closing(sqlite3.connect(self.database)) as connection:
            connection.execute(
                """
                INSERT INTO processing_run (
                    stage, input_version, output_version, started_at,
                    completed_at, fatal_batch, tool_versions, error_summary
                ) VALUES ('ingest', 'v1', 'v1', '2', '3', 1, '{}', 'failed')
                """
            )
            connection.commit()
        with self.assertRaisesRegex(BuildError, "incomplete or failed"):
            build_snapshot(self.data_dir)
        self.assertEqual(active_before, (self.data_dir / "active.json").read_bytes())

    def test_incomplete_ingest_cannot_replace_the_active_snapshot(self) -> None:
        build_snapshot(self.data_dir)
        active_before = (self.data_dir / "active.json").read_bytes()
        with closing(sqlite3.connect(self.database)) as connection:
            connection.execute(
                """
                INSERT INTO processing_run (
                    stage, input_version, output_version, started_at, tool_versions
                ) VALUES ('ingest', 'v1', 'v1', '2', '{}')
                """
            )
            connection.commit()
        with self.assertRaisesRegex(BuildError, "incomplete or failed"):
            build_snapshot(self.data_dir)
        self.assertEqual(active_before, (self.data_dir / "active.json").read_bytes())

    def test_unresolved_manifest_cannot_replace_the_active_snapshot(self) -> None:
        build_snapshot(self.data_dir)
        active_before = (self.data_dir / "active.json").read_bytes()
        with closing(sqlite3.connect(self.database)) as connection:
            connection.execute(
                "UPDATE corpus_state SET unresolved_manifest = 'failed-manifest'"
            )
            connection.commit()
        with self.assertRaisesRegex(BuildError, "remains unresolved"):
            build_snapshot(self.data_dir)
        self.assertEqual(active_before, (self.data_dir / "active.json").read_bytes())

    def test_post_commit_directory_fsync_reports_uncertain_durability(self) -> None:
        first = build_snapshot(self.data_dir)
        with closing(sqlite3.connect(self.database)) as connection:
            connection.execute("UPDATE meme_item SET title = 'New snapshot'")
            connection.commit()

        original = __import__(
            "mambomeme_search.build_index", fromlist=["_fsync_directory"]
        )._fsync_directory

        def fail_only_for_active_directory(path: Path) -> None:
            if path == self.data_dir:
                raise OSError("simulated post-commit fsync failure")
            original(path)

        with mock.patch(
            "mambomeme_search.build_index._fsync_directory",
            side_effect=fail_only_for_active_directory,
        ):
            with self.assertRaisesRegex(BuildError, "durability is unknown"):
                build_snapshot(self.data_dir)
        replacement = json.loads((self.data_dir / "active.json").read_text())
        self.assertNotEqual(first["snapshot_id"], replacement["snapshot_id"])

    def test_provenance_change_updates_dataset_identity(self) -> None:
        first = build_snapshot(self.data_dir)
        with closing(sqlite3.connect(self.database)) as connection:
            connection.execute(
                "UPDATE source_item SET attribution = 'Updated attribution'"
            )
            connection.commit()
        replacement = build_snapshot(self.data_dir)
        self.assertNotEqual(
            first["canonical_identity"], replacement["canonical_identity"]
        )


if __name__ == "__main__":
    unittest.main()
