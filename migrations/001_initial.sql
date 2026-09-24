PRAGMA foreign_keys = ON;

CREATE TABLE schema_version (
    version INTEGER PRIMARY KEY
);

INSERT INTO schema_version (version) VALUES (1);

CREATE TABLE corpus_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    unresolved_manifest TEXT
);

INSERT INTO corpus_state (singleton) VALUES (1);

CREATE TABLE meme_item (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('text', 'image')),
    title TEXT NOT NULL,
    body_text TEXT,
    asset_uri TEXT,
    language TEXT NOT NULL,
    content_hash TEXT NOT NULL UNIQUE,
    available INTEGER NOT NULL CHECK (available IN (0, 1)),
    safe INTEGER NOT NULL CHECK (safe IN (0, 1)),
    reviewed INTEGER NOT NULL CHECK (reviewed IN (0, 1)),
    people TEXT NOT NULL,
    template TEXT,
    tags TEXT NOT NULL,
    ocr TEXT,
    caption TEXT,
    description TEXT,
    processing_version TEXT NOT NULL,
    CHECK (
        (kind = 'text' AND body_text IS NOT NULL AND asset_uri IS NULL)
        OR (kind = 'image' AND asset_uri IS NOT NULL)
    )
);

CREATE TABLE source_item (
    source TEXT NOT NULL,
    source_item_id TEXT NOT NULL,
    meme_item_id TEXT,
    kind TEXT,
    source_url TEXT NOT NULL,
    media_url TEXT,
    creator TEXT,
    licence TEXT,
    permission TEXT,
    attribution TEXT,
    retention_policy TEXT,
    redistribution_policy TEXT,
    raw_payload_path TEXT,
    fetched_at TEXT NOT NULL,
    source_updated_at TEXT,
    content_hash TEXT,
    record_hash TEXT,
    canonical_json TEXT,
    safe INTEGER CHECK (safe IN (0, 1)),
    reviewed INTEGER CHECK (reviewed IN (0, 1)),
    outcome TEXT NOT NULL,
    reason TEXT,
    deleted INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0, 1)),
    PRIMARY KEY (source, source_item_id),
    FOREIGN KEY (meme_item_id) REFERENCES meme_item(id)
);

CREATE INDEX source_item_meme_item_idx ON source_item (meme_item_id);
CREATE INDEX source_item_outcome_idx ON source_item (outcome);

CREATE TABLE processing_run (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    stage TEXT NOT NULL,
    input_version TEXT NOT NULL,
    output_version TEXT NOT NULL,
    cursor TEXT,
    started_at TEXT NOT NULL,
    completed_at TEXT,
    accepted INTEGER NOT NULL DEFAULT 0,
    unchanged INTEGER NOT NULL DEFAULT 0,
    duplicate INTEGER NOT NULL DEFAULT 0,
    quarantined INTEGER NOT NULL DEFAULT 0,
    deleted INTEGER NOT NULL DEFAULT 0,
    fatal_batch INTEGER NOT NULL DEFAULT 0,
    tool_versions TEXT NOT NULL,
    error_summary TEXT
);
