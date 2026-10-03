---
description: Components, Rust and Python ownership, process boundaries, data flow, contracts, and repository shape.
title: Architecture
order: 10
---

::page{layout="docs" width="normal" sidebar=true}

# Architecture

## Product boundary

MamboMeme searches a local corpus of existing meme images and quotes. A user enters a query, inspects a ranked list, and selects an item. The first release does not invent a reply, generate an image, read a conversation, or autonomously act on the user's behalf.

The primary question is:

> Given this search query, which stored meme assets are most useful to show first?

For `john cena`, the engine should surface assets whose title, people, template, tags, OCR, or other metadata identify John Cena. Semantic evidence supplements this direct evidence; it does not redefine the query as a conversation.

## Four components

```text
1. DATA PROCESSING AND STORAGE
   source -> validated canonical item -> searchable corpus snapshot

2. RETRIEVAL
   query -> lexical and semantic candidates -> ranked results

3. USER INTERFACE
   search -> list -> preview -> selection

4. CONTEXT ASSISTANT — FUTURE
   one message or short chat -> search cues -> normal retrieval
```

The first three components form the first useful release. The fourth reuses them later; it is not a dependency of the search product.

Delivery uses four implementation phases rather than one phase per product component: Phase 1 creates an executable local corpus, Phase 2 creates ranked retrieval and evaluation, Phase 3 creates the TUI and first release, and Phase 4 adds one approved external source. See the [Roadmap](Roadmap.md).

## Language ownership

The full system keeps one owner for each responsibility:

| Rust application | Python search package |
|---|---|
| Source acquisition, first-pass trust-boundary validation, hashing, provenance, exact deduplication, staging, terminal lifecycle, TUI state, result presentation, and optional local interaction-event capture | Resource-limited OCR and annotations, search-document construction, embeddings, FTS and dense retrieval, rank fusion, filters, evaluation, and the long-lived search worker |

Rust is useful for a distributable terminal program, careful handling of source bytes, and systems practice. Python keeps the ML ecosystem close to the model and evaluation code. This split is not a blanket claim that Rust makes every operation faster:

- SQLite FTS executes inside SQLite whichever language submits the query.
- NumPy and model frameworks already perform heavy numeric work in native kernels.
- Model encoding is likely to dominate search latency on a small local corpus.

Keep one implementation of each responsibility. Measure before moving a boundary.

## Current implementation boundary

Phases 1 through 3 are complete; Phase 4 is in progress. Phase 1 established the durable corpus boundary:

```text
cleared local JSONL manifest and static fixture assets
    -> Rust sets an unresolved-manifest gate, validates, hashes, deduplicates, and commits one manifest transaction
    -> successful commit clears the gate; failure blocks other manifests and publication
    -> Python builds deterministic fielded FTS5 in a candidate snapshot
    -> Python validates integrity and coverage, checksums artifacts, and atomically publishes active.json
```

Phase 2 extends that published snapshot without changing ownership:

```text
bounded cues + kind/language filters
    -> weighted FTS5/BM25 lexical route
    -> optional TF-IDF/LSA exact-cosine route
    -> deterministic reciprocal-rank fusion and stable ties
    -> ranked items through a strict UTF-8 NDJSON worker
    -> provisional fixture evaluator and performance report
```

Phase 3 completes the local interaction path:

```text
explicit query submission in a Ratatui/Crossterm TUI
    -> one background Rust controller talks to one Python worker per session
    -> validated results render as a ranked list plus portable metadata preview
    -> open, manual-copy, or explicit stable-ID selection
    -> optional private JSONL interaction event
    -> bounded shutdown and terminal restoration
```

Phase 4 extends only the acquisition side:

```text
finite human-reviewed Wikimedia Commons page-ID plan
    -> fixed Action API metadata request and verified original-media download
    -> pinned content, identity, rights, safety, and display-metadata comparison
    -> resumable raw/media/source-envelope staging
    -> the existing Rust ingest and Python publication boundary
    -> explicit tombstone or changed-rights replay invalidates active.json
```

The initial migration, cleared fixture data, Rust commands, Python builder, retrieval routes, evaluator, protocol types, worker, TUI, feedback log, profiler, unit tests, and offline cross-language fixtures are implemented. Phase 4 is adding bounded static JPEG and PNG validation alongside the existing PPM fixture decoder and one concrete Commons client; it does not add a general URL fetcher.

There are still no OCR calls, pretrained model downloads, image embeddings, inline terminal images, platform clipboard writes, uploaded telemetry, or context processing. The only planned network boundary is the allowlisted Commons metadata and original-media acquisition described in [Wikimedia Commons source](Wikimedia%20Commons%20Source.md).

## Full corpus build

```text
approved API or cleared local manifest
    -> Rust fetches or reads, validates, hashes, deduplicates, and stages
    -> raw content-addressed assets + SQLite provenance rows
    -> Python performs constrained OCR and enrichment only when a source needs them
    -> Python builds fielded FTS documents and declared dense representations
    -> Python validates and atomically publishes a corpus snapshot
```

The stages do not write concurrently. Rust completes its acquisition transaction before Python enrichment begins. The interactive search worker opens only the published snapshot and treats it as read-only.

Phase 2 adds a model-free TF-IDF/LSA representation and retrieval. Pretrained text/image models remain later measured experiments; Phase 4 adds source acquisition without changing retrieval ownership.

## Interactive search — Phases 2 and 3

```text
user submits query in Rust TUI
    -> TUI sends versioned NDJSON search request
    -> long-lived Python worker validates and normalizes the query
    -> BM25 lexical candidates + dense semantic candidates
    -> deterministic fusion, eligibility filters, and duplicate collapse
    -> NDJSON ranked-result response
    -> TUI renders list and selected-item preview
    -> user opens, copies, or selects one item
```

Phase 2 creates the worker and headless retrieval. Phase 3 starts one worker per TUI session so the index and corpus load once. Search runs on a controller thread while the terminal event loop remains responsive. There is no HTTP server, embedded Python, per-query process, or duplicate Rust search implementation in the source release.

## Online protocol — implemented in Phase 2

Use UTF-8 newline-delimited JSON over the worker's standard input and standard output. Standard error is diagnostic output only. Every message has `protocol_version`, `type`, and `request_id` where applicable.

The worker receives the corpus path as a launch argument, validates the complete snapshot, and emits `ready`. No extra `hello` message repeats command-line configuration. The minimal sequence is:

```text
Rust                              Python
 |       starts worker ------------>|
 |<----- ready ---------------------|
 |------ search(request_id) -------->|
 |<----- results(request_id) -------|
 |------ shutdown ----------------->|
 |<----- bye -----------------------|
```

Required message types:

| Type | Direction | Purpose |
|---|---|---|
| `ready` | Python to Rust | Confirm compatible protocol, loaded corpus, and retriever versions. |
| `search` | Rust to Python | Submit bounded query cues, filters, and result limit. |
| `results` | Python to Rust | Return ranked items and route evidence. |
| `error` | Python to Rust | Return a typed request-scoped or fatal failure. |
| `shutdown` | Rust to Python | Request deliberate session closure. |
| `bye` | Python to Rust | Confirm deliberate closure. |

The implemented worker bounds input lines at 64 KiB and output lines at 16 MiB, writes UTF-8 bytes independent of the process locale, accepts one request at a time, rejects duplicate request IDs, and distinguishes recoverable request errors from fatal framing, protocol, artifact, and search errors. Oversized input fails immediately without waiting for a newline. A result list that would exceed the output bound drops tail results and sets `truncated`; one individually oversized result is fatal. Invalid JSON is recoverable; an oversized or partial line, protocol mismatch, premature input EOF, or internal search failure emits a fatal error and exits non-zero. A valid `shutdown` produces `bye` and exit zero.

The Phase 3 Rust client validates the ready handshake, request ID, corpus/retriever versions, requested and announced routes, result count, rank sequence, safe flag, and non-empty unique result IDs. Each protocol response has a five-second timeout. The UI permits one in-flight submission, treats request validation errors as recoverable, treats worker/protocol failures as fatal, and offers at most one deliberate `r` restart per session. A stale or mismatched response is rejected before it can replace the displayed result set.

Phase 4 also closes the stale-process path. The Python engine rereads the bounded active pointer before every search and requires the snapshot and dataset identities it opened. Removing `active.json` after a tombstone or serving-provenance change, or publishing another snapshot, stops that worker instead of allowing its already-open SQLite handle to continue serving stale content.

## Artifact contract

The table describes the shared release contract. Phases 1 and 2 implement the raw-media, SQLite, migration, published-manifest, FTS, and LSA rows; Phase 3 implements the optional interaction events; Phase 4 adds source-acquisition artifacts without making them a second canonical database.

Rust and Python exchange durable, inspectable build artifacts:

| Artifact | Owner | Consumer |
|---|---|---|
| Reviewed source plan, retained bounded API payload, cursor, and acquisition report | Rust acquisition command writes | Human review and Rust ingestion consume |
| Content-addressed raw media | Rust writes | Python reads within the enrichment sandbox |
| Source, rights, outcome, and processing rows in SQLite | Rust writes | Python enriches during a stopped build |
| Ordered, language-neutral SQL migrations | Shared contract | Both apply or inspect |
| Field-labelled search documents and FTS tables | Python writes | Python worker reads |
| `dense_ids.json`, vocabulary/IDF files, SVD components, and L2-normalized dense vectors | Python writes | Python worker reads |
| Published manifest and checksums | Python writes | Python worker verifies at startup |
| Optional interaction events in versioned JSON Lines | Rust writes | Offline Python analysis reads |

The manifest records schema, builder, normalization, search-document, representation, NumPy and SQLite versions; canonical content identity; dense method and dimension; item/vocabulary counts; and every artifact checksum. Corpus identity is computed from a canonical ordered export rather than SQLite page bytes or timestamps. The snapshot identity also protects the derived artifact manifest.

The Phase 1 portion of one fixed fixture proves ingestion, outcomes, publication, and reproducibility. Phases 2 and 3 extend the same fixture through retrieval and selection; Phase 4 adds a controlled-server acquisition and tombstone replay:

```text
Rust fixture import
    -> expected accepted, duplicate, and quarantine outcomes
    -> Python FTS index build and atomic publication
    -> Phase 2 worker returns the development-only "john cena" fixture at rank 1
    -> both languages decode the same golden protocol messages
    -> a second build preserves content identity and ranking
    -> Phase 3 Rust TUI receives and selects the intended stable ID
    -> Phase 4 source record enters the same index, then a tombstone invalidates publication and the rebuilt corpus excludes it
```

## Search request

| Field | Contract |
|---|---|
| `request_id` | Session-unique identifier used to pair responses. |
| `cues` | One to four non-empty search cues, each at most 512 UTF-8 bytes. Multiple cues are alternate OR-style hints, not conversation turns. |
| `limit` | Positive result count, default `10`, maximum `50`. |
| `filters.kind` | Optional `text` or `image` filter. |
| `filters.language` | Optional simple language tag; omission means no language filter. |
| `route` | `lexical` by default; `dense` and `hybrid` are explicit experiment routes. |

## Search result

| Field | Contract |
|---|---|
| `id`, `kind` | Stable corpus identity and item type. |
| `title`, `text`, `asset_uri` | Display fields; text and image items have different required fields. |
| `caption` | Portable preview material when available. |
| `people`, `template`, `tags` | Searchable identity and grouping metadata. |
| `source`, `attribution` | Provenance required by the source policy. |
| `matched_fields`, `routes` | Explain whether names, tags, OCR, lexical, or dense evidence contributed. |
| `rank` | Final one-based position; internal scores are not probabilities. |
| `scores` | Diagnostic lexical/dense ranks and fused evidence, never a probability. |
| `truncated` | Worker-envelope flag showing that tail results were removed to satisfy the output bound. |
| `dataset_version`, `retriever_version` | Reproducibility identifiers. |

An empty result list is a successful search outcome. Invalid input or worker failure is a typed error, never disguised as an empty search.

## Storage boundary

```text
permitted raw assets and payloads    content-addressed local files      Phase 1
canonical metadata and FTS           SQLite                            Phase 1
active corpus                         checksummed manifest pointer       Phase 1
TF-IDF/LSA experiment artifacts      NumPy matrices + ordered JSON IDs  Phase 2
benchmark and reports                 versioned JSON/Markdown            Phase 2
selection feedback                    optional local JSONL               Phase 3
source plan/cursor/raw/report         versioned files in acquisition dir Phase 4
```

Raw, canonical, and derived data are layers of one corpus, not three competing sources of truth. Derived FTS and embedding artifacts can be rebuilt. A model change produces a new manifest and never overwrites the artifacts attached to an earlier score.

## Repository shape by phase

The project keeps this single-package shape through Phase 4:

```text
README.md
docs/
Cargo.toml
Cargo.lock
rust-toolchain.toml
migrations/
    001_initial.sql
src/
    main.rs                  acquisition, ingest, TUI, feedback, and profile dispatch
    commons.rs               one reviewed Commons acquisition path
    ingest.rs                local validation, storage, and outcomes
    protocol.rs              strict Rust NDJSON message types
    worker.rs                bounded Python process and response validation
    tui.rs                   state, keys, result/preview rendering, and help
    interface.rs             terminal guard, controller, actions, and lifecycle
    feedback.rs              opt-in private JSONL events and retention
    profile.rs               release-mode PTY submit-to-render profile
pyproject.toml
python/mambomeme_search/
    build_index.py           deterministic FTS/LSA build and publication
    dense.py                 TF-IDF/LSA artifact build and exact cosine scan
    retrieve.py              validation, routes, fusion, and result contract
    worker.py                long-lived strict NDJSON process
    evaluate.py              metrics, comparison, and report generation
    text.py                  shared search normalization and tokenization
benchmarks/
    provisional-v1.json
    reports/phase2-provisional.{json,md}
    reports/phase3-interface.json
    reports/phase4-wikimedia.json
examples/
    wikimedia-commons-plan.json
tests/
    fixtures/corpus/         cleared local manifest and assets
    fixtures/protocol/       cross-language golden messages
    python/                  index, retrieval, worker, and evaluator tests
    test_phase1.py           offline Rust-to-Python acceptance check
    test_phase2.py           offline corpus-to-worker acceptance check
    test_phase3.py           offline PTY selection, feedback, failure, and profile
    test_phase4.py           offline source-to-tombstone acceptance check
data/                        ignored working and published artifacts
```

Phase 4 adds one concrete source integration. One Cargo package and one Python package remain enough; do not add a Cargo workspace, web service, message queue, vector service, generic adapter framework, or empty context package.

## Failure behavior

| Failure | Required behavior |
|---|---|
| Invalid or rights-incomplete source item | Quarantine the item without losing the batch. |
| Commons content, identity, dimension, or rights drift | Quarantine the new revision; tombstone a previously served revision until it is reviewed. |
| Explicit Commons deletion or completed-plan removal | Emit a minimal tombstone, invalidate the active pointer after ingestion, then rebuild before serving. |
| API omission, interruption, or exhausted transient failure | Preserve the cursor and prior availability; never infer a deletion. |
| Rust/Python schema mismatch | Stop the build or worker startup. |
| Corrupt manifest or item/vector mismatch | Refuse publication or startup. |
| Dense artifact missing or corrupt at startup | Reject the complete Phase 2 snapshot; lexical-only degradation is allowed only for a runtime dense-route failure after successful startup. |
| FTS unavailable in a scored run | Fail that run; do not silently change the evaluated system. |
| Worker crash or timeout | Preserve terminal control, show an error, and allow an explicit restart. |
| Unsupported terminal image protocol | Use the text/metadata preview and external-open action. |
| No eligible match | Return an empty result list. |
| Permission revocation or active-pointer change | Stop the open worker and serving snapshot until a replacement is built and the worker restarts. |

## Deferred boundary

The future context assistant may accept one sentence or a bounded sequence of role-labelled chat messages. It will derive several short search cues and pass them through the same retrieval contract. Its model, privacy rules, prompt-injection handling, and evaluation belong to a separate benchmark and are not part of the first repository implementation.
